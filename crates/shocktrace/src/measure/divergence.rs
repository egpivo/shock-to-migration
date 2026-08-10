//! Divergence between two assets' daily returns on matched trading days.
//!
//! `D_t = r_A,t - r_B,t`, computed only on the **intersection** of days
//! where both assets have a computable daily return. There is no
//! forward-fill: a session observed for A but not B (or vice versa) is
//! excluded from the divergence series entirely, never treated as a zero
//! return for the missing side.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::coverage::EvidenceBoundary;
use crate::event::EventWindow;
use crate::identity::AssetKey;
use crate::measure::project_support::{
    build_measure_boundary, load_response_rows, require_asset_observations,
    resolve_baseline_window, resolve_event_window, MeasureProjectError,
};
use crate::measure::returns::{returns_from_sorted, sorted_validated, MeasureError};
use crate::measure::stats::{compute_z_score, ZScoreUnavailable};
use crate::project::ProjectConfig;
use crate::response::ResponseObservation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DivergenceSummary {
    pub asset_a: AssetKey,
    pub asset_b: AssetKey,
    pub event_window_name: String,
    pub baseline_window_name: String,
    /// Total number of days (across the whole series, not just baseline)
    /// where both assets have a computable daily return.
    pub matched_days: usize,
    pub event_day: Option<NaiveDate>,
    pub event_divergence: Option<Decimal>,
    pub baseline_n: usize,
    pub baseline_mean: Option<Decimal>,
    pub baseline_std: Option<Decimal>,
    pub z_score: Option<Decimal>,
    /// True when z was computed but baseline_n is below the adequacy floor.
    pub low_baseline: bool,
    pub unavailable_reason: Option<ZScoreUnavailable>,
    pub interpretation_boundary: &'static str,
    pub boundary: EvidenceBoundary,
}

const DIVERGENCE_INTERPRETATION_BOUNDARY: &str = "Divergence is a descriptive comparison of two assets' daily returns on matched trading days only (no forward-fill for unmatched sessions). It is not a cointegration test, pairs-trading signal, or migration inference.";

/// # Errors
/// Empty series, mixed `asset_key`s, or duplicate `(asset_key, day)` rows,
/// for either asset's observations.
pub fn account_divergence(
    obs_a: Vec<ResponseObservation>,
    obs_b: Vec<ResponseObservation>,
    event_window: &EventWindow,
    baseline_window: &EventWindow,
) -> Result<DivergenceSummary, MeasureError> {
    let (asset_a, sorted_a) = sorted_validated(obs_a)?;
    let (asset_b, sorted_b) = sorted_validated(obs_b)?;

    let returns_a = returns_from_sorted(&sorted_a);
    let returns_b = returns_from_sorted(&sorted_b);

    let map_b: BTreeMap<NaiveDate, Decimal> = returns_b.iter().map(|r| (r.day, r.value)).collect();
    let divergences: BTreeMap<NaiveDate, Decimal> = returns_a
        .iter()
        .filter_map(|ra| map_b.get(&ra.day).map(|rb| (ra.day, ra.value - *rb)))
        .collect();

    let matched_days = divergences.len();

    // BTreeMap iterates in ascending key (day) order, so this is the
    // earliest matched day inside the event window.
    let event_point = divergences
        .iter()
        .find(|(day, _)| event_window.contains(**day));
    let event_day = event_point.map(|(d, _)| *d);
    let event_divergence = event_point.map(|(_, v)| *v);

    let baseline_values: Vec<Decimal> = divergences
        .iter()
        .filter(|(day, _)| baseline_window.contains(**day))
        .map(|(_, v)| *v)
        .collect();

    let z = compute_z_score(event_divergence, &baseline_values);

    Ok(DivergenceSummary {
        asset_a,
        asset_b,
        event_window_name: event_window.name.clone(),
        baseline_window_name: baseline_window.name.clone(),
        matched_days,
        event_day,
        event_divergence,
        baseline_n: z.baseline_n,
        baseline_mean: z.baseline_mean,
        baseline_std: z.baseline_std,
        z_score: z.z_score,
        low_baseline: z.low_baseline,
        unavailable_reason: z.unavailable_reason,
        interpretation_boundary: DIVERGENCE_INTERPRETATION_BOUNDARY,
        boundary: EvidenceBoundary::default(),
    })
}

/// Loads both assets' response series from the project and computes a
/// [`DivergenceSummary`]. Window selection follows the same convention as
/// [`crate::measure::shock::load_asset_shock_report`].
pub fn load_divergence_summary(
    cfg: &ProjectConfig,
    asset_a: &AssetKey,
    asset_b: &AssetKey,
    event_window_override: Option<&str>,
    baseline_window_override: Option<&str>,
) -> Result<DivergenceSummary, MeasureProjectError> {
    let rows = load_response_rows(cfg)?;
    let series_a = require_asset_observations(cfg, &rows, asset_a)?;
    let series_b = require_asset_observations(cfg, &rows, asset_b)?;

    let baseline_window = resolve_baseline_window(cfg, baseline_window_override)?;
    let event_window = resolve_event_window(cfg, event_window_override, &baseline_window.name)?;

    let mut summary = account_divergence(
        series_a.clone(),
        series_b.clone(),
        event_window,
        baseline_window,
    )?;
    summary.boundary = build_measure_boundary(
        cfg,
        &[asset_a, asset_b],
        &[baseline_window, event_window],
        &[
            (asset_a, series_a.as_slice()),
            (asset_b, series_b.as_slice()),
        ],
        summary.low_baseline,
        summary.baseline_n,
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::default_window_applies_to;
    use std::str::FromStr;

    fn obs(asset: &str, day: &str, price: &str) -> ResponseObservation {
        ResponseObservation {
            asset_key: AssetKey::new(asset),
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

    #[test]
    fn aligned_divergence_and_z_score() {
        // A returns: 0.02, 0.04, 0.02, 0.04 (baseline), 0.10 (event)
        // B returns: 0.01, 0.01, 0.01, 0.01 (baseline), 0.01 (event)
        // D_t (A-B): 0.01, 0.03, 0.01, 0.03 (baseline) -> mean 0.02, std 0.01
        // Event D = 0.09 -> z = (0.09 - 0.02) / 0.01 = 7
        let a = vec![
            obs("A", "2026-01-01", "100"),
            obs("A", "2026-01-02", "102"),         // 0.02
            obs("A", "2026-01-03", "106.08"),      // 0.04
            obs("A", "2026-01-04", "108.2016"),    // 0.02
            obs("A", "2026-01-05", "112.529664"),  // 0.04
            obs("A", "2026-01-06", "123.7826304"), // 0.10 (event)
        ];
        let b = vec![
            obs("B", "2026-01-01", "50"),
            obs("B", "2026-01-02", "50.5"),         // 0.01
            obs("B", "2026-01-03", "51.005"),       // 0.01
            obs("B", "2026-01-04", "51.51505"),     // 0.01
            obs("B", "2026-01-05", "52.0302005"),   // 0.01
            obs("B", "2026-01-06", "52.550502505"), // 0.01 (event)
        ];
        let baseline = window("baseline", "2026-01-02", "2026-01-05");
        let event = window("event", "2026-01-06", "2026-01-06");
        let summary = account_divergence(a, b, &event, &baseline).unwrap();
        assert_eq!(summary.matched_days, 5);
        assert_eq!(summary.event_day.unwrap().to_string(), "2026-01-06");
        assert_eq!(
            summary.event_divergence,
            Some(Decimal::from_str("0.09").unwrap())
        );
        assert_eq!(summary.baseline_n, 4);
        assert_eq!(
            summary.baseline_mean,
            Some(Decimal::from_str("0.02").unwrap())
        );
        assert_eq!(
            summary.baseline_std,
            Some(Decimal::from_str("0.01").unwrap())
        );
        assert_eq!(summary.z_score, Some(Decimal::from_str("7").unwrap()));
    }

    #[test]
    fn missing_day_one_side_is_excluded_not_zero_filled() {
        let a = vec![
            obs("A", "2026-01-01", "100"),
            obs("A", "2026-01-02", "101"),
            obs("A", "2026-01-03", "102"),
        ];
        // B is missing 2026-01-03 entirely.
        let b = vec![obs("B", "2026-01-01", "50"), obs("B", "2026-01-02", "50.5")];
        let baseline = window("baseline", "2026-01-02", "2026-01-02");
        let event = window("event", "2026-01-03", "2026-01-03");
        let summary = account_divergence(a, b, &event, &baseline).unwrap();
        assert_eq!(summary.matched_days, 1);
        assert!(summary.event_divergence.is_none());
        assert_eq!(
            summary.unavailable_reason,
            Some(ZScoreUnavailable::NoEventValue)
        );
    }

    #[test]
    fn rejects_duplicate_date_on_either_side() {
        let a = vec![obs("A", "2026-01-01", "100"), obs("A", "2026-01-01", "101")];
        let b = vec![obs("B", "2026-01-01", "50")];
        let baseline = window("baseline", "2026-01-01", "2026-01-01");
        let event = window("event", "2026-01-01", "2026-01-01");
        let err = account_divergence(a, b, &event, &baseline).unwrap_err();
        assert!(matches!(err, MeasureError::DuplicateObservation { .. }));
    }
}
