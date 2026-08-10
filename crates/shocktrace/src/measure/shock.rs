//! Shock score: a z-score on one asset's event-day daily return.
//!
//! **Convention (v1, deliberately minimal):** the event metric is the daily
//! simple return on the first day inside the event window that has a
//! computable daily return (not a compounded/cumulative window return). It
//! is compared to the mean/std of daily returns inside the baseline window.
//! Multi-day cumulative event effects are covered separately by
//! [`crate::measure::horizon::cumulative_return_from_event`], which is
//! horizon-indexed rather than z-scored. This split keeps one thing
//! measured per function instead of overloading a single number.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::coverage::EvidenceBoundary;
use crate::event::EventWindow;
use crate::identity::AssetKey;
use crate::measure::activity::{account_activity_anomaly, ActivityAnomaly};
use crate::measure::horizon::{cumulative_return_from_event, HorizonReturns};
use crate::measure::project_support::{
    build_measure_boundary, load_response_rows, require_asset_observations,
    resolve_baseline_window, resolve_event_window, MeasureProjectError,
};
use crate::measure::returns::{returns_from_sorted, sorted_validated, MeasureError};
use crate::measure::stats::{compute_z_score, ZScoreUnavailable};
use crate::project::ProjectConfig;
use crate::response::ResponseObservation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShockScore {
    pub asset_key: AssetKey,
    pub event_window_name: String,
    pub baseline_window_name: String,
    /// The day the event-day return is attributed to (first day-with-return
    /// inside the event window), if any.
    pub event_day: Option<NaiveDate>,
    /// Daily simple return on `event_day`.
    pub event_return: Option<Decimal>,
    pub baseline_n: usize,
    pub baseline_mean: Option<Decimal>,
    pub baseline_std: Option<Decimal>,
    pub z_score: Option<Decimal>,
    /// True when z was computed but baseline_n is below the adequacy floor.
    pub low_baseline: bool,
    pub unavailable_reason: Option<ZScoreUnavailable>,
    pub interpretation_boundary: &'static str,
}

const SHOCK_INTERPRETATION_BOUNDARY: &str = "Shock score is a descriptive z-score of one asset's event-day daily return against its own baseline-window daily-return distribution. It is not a statistical-significance test, not causal inference, and not a migration signal.";

/// Computes a [`ShockScore`] from one asset's full observation series (any
/// order, any windows present). Daily returns are computed across the whole
/// series *before* filtering by window membership, so a return whose price
/// pair spans a window boundary still attributes correctly to its later day.
///
/// # Errors
/// Empty series, mixed `asset_key`s, or duplicate `(asset_key, day)` rows.
pub fn account_shock_score(
    observations: Vec<ResponseObservation>,
    event_window: &EventWindow,
    baseline_window: &EventWindow,
) -> Result<ShockScore, MeasureError> {
    let (asset_key, sorted) = sorted_validated(observations)?;
    let returns = returns_from_sorted(&sorted);

    // `returns` is ordered by `day` ascending (built from a sorted series),
    // so the first match is the earliest day-with-return in the window.
    let event_point = returns.iter().find(|r| event_window.contains(r.day));
    let event_day = event_point.map(|r| r.day);
    let event_return = event_point.map(|r| r.value);

    let baseline_values: Vec<Decimal> = returns
        .iter()
        .filter(|r| baseline_window.contains(r.day))
        .map(|r| r.value)
        .collect();

    let z = compute_z_score(event_return, &baseline_values);

    Ok(ShockScore {
        asset_key,
        event_window_name: event_window.name.clone(),
        baseline_window_name: baseline_window.name.clone(),
        event_day,
        event_return,
        baseline_n: z.baseline_n,
        baseline_mean: z.baseline_mean,
        baseline_std: z.baseline_std,
        z_score: z.z_score,
        low_baseline: z.low_baseline,
        unavailable_reason: z.unavailable_reason,
        interpretation_boundary: SHOCK_INTERPRETATION_BOUNDARY,
    })
}

/// Bundled shock-tool output for one asset: event-day z-score, horizon
/// returns anchored at the event window's start, and an activity anomaly
/// comparing the event window's volume to the baseline window's volume.
// Serialize only (not Deserialize): nesting structs that carry a
// `&'static str` field (`interpretation_boundary`) inside another struct
// hits a serde-derive lifetime limitation for `Deserialize`. This bundle is
// CLI output only, never round-tripped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetShockReport {
    pub shock: ShockScore,
    pub horizons: HorizonReturns,
    pub activity: ActivityAnomaly,
    pub boundary: EvidenceBoundary,
}

/// Loads one asset's response series from the project and computes the full
/// [`AssetShockReport`] (shock score + horizon returns + activity anomaly).
///
/// Window selection: `baseline_window_override` defaults to
/// `[response].baseline_window`; `event_window_override` defaults to the
/// first declared window with `applies_to` including `response`, excluding
/// the baseline window. See `project_support` module docs for why this is a
/// separate surface from `respond`.
pub fn load_asset_shock_report(
    cfg: &ProjectConfig,
    asset: &AssetKey,
    event_window_override: Option<&str>,
    baseline_window_override: Option<&str>,
    horizon_sessions: &[usize],
) -> Result<AssetShockReport, MeasureProjectError> {
    let rows = load_response_rows(cfg)?;
    let series = require_asset_observations(cfg, &rows, asset)?;

    let baseline_window = resolve_baseline_window(cfg, baseline_window_override)?;
    let event_window = resolve_event_window(cfg, event_window_override, &baseline_window.name)?;

    let shock = account_shock_score(series.clone(), event_window, baseline_window)?;
    let horizons =
        cumulative_return_from_event(series.clone(), event_window.start, horizon_sessions)?;

    let event_obs: Vec<_> = series
        .iter()
        .filter(|o| event_window.contains(o.day))
        .cloned()
        .collect();
    let baseline_obs: Vec<_> = series
        .iter()
        .filter(|o| baseline_window.contains(o.day))
        .cloned()
        .collect();
    let activity = account_activity_anomaly(
        asset.clone(),
        event_window.name.clone(),
        baseline_window.name.clone(),
        event_obs,
        baseline_obs,
    )?;

    let boundary = build_measure_boundary(
        cfg,
        &[asset],
        &[baseline_window, event_window],
        &[(asset, series.as_slice())],
        shock.low_baseline,
        shock.baseline_n,
    );

    Ok(AssetShockReport {
        shock,
        horizons,
        activity,
        boundary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::default_window_applies_to;
    use std::str::FromStr;

    fn obs(day: &str, price: &str) -> ResponseObservation {
        ResponseObservation {
            asset_key: AssetKey::new("GC"),
            day: NaiveDate::from_str(day).unwrap(),
            price: Some(Decimal::from_str(price).unwrap()),
            volume: None,
        }
    }

    fn window(name: &str, start: &str, end: &str) -> EventWindow {
        EventWindow {
            name: name.into(),
            start: NaiveDate::from_str(start).unwrap(),
            end: NaiveDate::from_str(end).unwrap(),
            applies_to: default_window_applies_to(),
        }
    }

    /// Baseline daily returns 0.01, 0.03, 0.01, 0.03 -> mean 0.02, population
    /// std 0.01 (exact). Event return 0.05 -> z = (0.05 - 0.02) / 0.01 = 3.
    fn fixture_series() -> Vec<ResponseObservation> {
        vec![
            obs("2026-01-01", "100"),          // anchor, no return
            obs("2026-01-02", "101"),          // r = 0.01 (baseline)
            obs("2026-01-03", "104.03"),       // r = 0.03 (baseline)
            obs("2026-01-04", "105.0703"),     // r = 0.01 (baseline)
            obs("2026-01-05", "108.222409"),   // r = 0.03 (baseline)
            obs("2026-01-06", "113.63352945"), // r = 0.05 (event day)
        ]
    }

    #[test]
    fn known_z_score_fixture() {
        let baseline = window("baseline", "2026-01-02", "2026-01-05");
        let event = window("event", "2026-01-06", "2026-01-06");
        let score = account_shock_score(fixture_series(), &event, &baseline).unwrap();
        assert_eq!(score.event_day.unwrap().to_string(), "2026-01-06");
        assert_eq!(score.event_return, Some(Decimal::from_str("0.05").unwrap()));
        assert_eq!(score.baseline_n, 4);
        assert_eq!(
            score.baseline_mean,
            Some(Decimal::from_str("0.02").unwrap())
        );
        assert_eq!(score.baseline_std, Some(Decimal::from_str("0.01").unwrap()));
        assert_eq!(score.z_score, Some(Decimal::from_str("3").unwrap()));
        assert!(score.low_baseline); // n=4 < adequacy floor
        assert!(score.unavailable_reason.is_none());
    }

    #[test]
    fn thin_baseline_computes_z_but_flags_low_baseline() {
        let series = vec![
            obs("2026-01-01", "100"),
            obs("2026-01-02", "101"),    // r = 0.01
            obs("2026-01-03", "104.03"), // r = 0.03
            obs("2026-01-04", "110"),    // event
        ];
        let baseline = window("baseline", "2026-01-02", "2026-01-03");
        let event = window("event", "2026-01-04", "2026-01-04");
        let score = account_shock_score(series, &event, &baseline).unwrap();
        assert!(score.z_score.is_some());
        assert!(score.low_baseline);
        assert_eq!(score.baseline_n, 2);
    }

    #[test]
    fn zero_variance_baseline_yields_none_with_reason() {
        let series = vec![
            obs("2026-01-01", "100"),
            obs("2026-01-02", "102"),    // r = 0.02
            obs("2026-01-03", "104.04"), // r = 0.02
            obs("2026-01-04", "110"),    // event day, r ~ 0.0573
        ];
        let baseline = window("baseline", "2026-01-02", "2026-01-03");
        let event = window("event", "2026-01-04", "2026-01-04");
        let score = account_shock_score(series, &event, &baseline).unwrap();
        assert!(score.z_score.is_none());
        assert_eq!(
            score.unavailable_reason,
            Some(ZScoreUnavailable::ZeroBaselineVariance)
        );
    }

    #[test]
    fn insufficient_baseline_yields_none_with_reason() {
        let series = vec![
            obs("2026-01-01", "100"),
            obs("2026-01-02", "101"), // single baseline return
            obs("2026-01-03", "110"), // event day
        ];
        let baseline = window("baseline", "2026-01-02", "2026-01-02");
        let event = window("event", "2026-01-03", "2026-01-03");
        let score = account_shock_score(series, &event, &baseline).unwrap();
        assert!(score.z_score.is_none());
        assert_eq!(
            score.unavailable_reason,
            Some(ZScoreUnavailable::InsufficientBaseline { have: 1, need: 2 })
        );
    }

    #[test]
    fn missing_event_observation_yields_none_with_reason() {
        let series = vec![
            obs("2026-01-01", "100"),
            obs("2026-01-02", "101"),
            obs("2026-01-03", "104.03"),
            // No observations at all inside the event window.
        ];
        let baseline = window("baseline", "2026-01-02", "2026-01-03");
        let event = window("event", "2026-06-01", "2026-06-01");
        let score = account_shock_score(series, &event, &baseline).unwrap();
        assert!(score.event_return.is_none());
        assert!(score.event_day.is_none());
        assert!(score.z_score.is_none());
        assert_eq!(
            score.unavailable_reason,
            Some(ZScoreUnavailable::NoEventValue)
        );
    }

    #[test]
    fn propagates_duplicate_day_error() {
        let series = vec![obs("2026-01-01", "100"), obs("2026-01-01", "101")];
        let baseline = window("baseline", "2026-01-01", "2026-01-01");
        let event = window("event", "2026-01-01", "2026-01-01");
        let err = account_shock_score(series, &event, &baseline).unwrap_err();
        assert!(matches!(err, MeasureError::DuplicateObservation { .. }));
    }
}
