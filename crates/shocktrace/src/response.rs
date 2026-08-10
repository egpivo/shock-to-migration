//! Market-response accounting (separate from directional flow).
//!
//! Metric definition: `market_response.v1`.
//!
//! Answers what changed in observed series around a window — not why.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{EventWindow, SessionCalendar};
use crate::identity::AssetKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseObservation {
    pub asset_key: AssetKey,
    pub day: NaiveDate,
    /// Missing price stays missing — never coerced to zero.
    pub price: Option<Decimal>,
    /// Missing volume stays missing — never coerced to zero.
    pub volume: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseSeriesSummary {
    pub asset_key: AssetKey,
    pub window_name: String,
    pub observed_days: usize,
    /// Expected session count under the asset's [`SessionCalendar`].
    pub window_days: usize,
    pub days_with_price: usize,
    pub days_with_volume: usize,
    /// `(last_price / first_price) - 1` over in-window days with price, ordered by day.
    /// `None` if fewer than two priced observations.
    pub price_return: Option<Decimal>,
    pub first_price: Option<Decimal>,
    pub last_price: Option<Decimal>,
    /// Median in-window volume / median of the configured baseline window's volumes.
    ///
    /// When the summarized window *is* the baseline window, the engine leaves this
    /// `None` (self-normalization is definitionally 1 and not informative).
    /// `None` also when either median is unavailable or baseline median ≤ 0.
    pub baseline_normalized_volume: Option<Decimal>,
    pub window_median_volume: Option<Decimal>,
    pub baseline_median_volume: Option<Decimal>,
    pub interpretation_boundary: &'static str,
}

#[derive(Debug, Error)]
pub enum ResponseError {
    #[error("empty response series for asset {0}")]
    EmptySeries(String),
    #[error("multiple asset_keys in one series (first={first}, also={other})")]
    MixedAssets { first: String, other: String },
    #[error("duplicate response observation for asset {asset} on {day}")]
    DuplicateObservation { asset: String, day: NaiveDate },
}

/// Deterministic market-response summary for one asset inside one window.
///
/// `session_calendar` selects the coverage denominator (`window_days`):
/// continuous calendar days vs Monday–Friday exchange sessions.
pub fn account_market_response(
    mut observations: Vec<ResponseObservation>,
    window: &EventWindow,
    baseline: &[ResponseObservation],
    session_calendar: SessionCalendar,
) -> Result<ResponseSeriesSummary, ResponseError> {
    if observations.is_empty() {
        return Err(ResponseError::EmptySeries("<unknown>".into()));
    }

    observations.sort_by_key(|o| o.day);
    let asset_key = observations[0].asset_key.clone();
    for obs in &observations {
        if obs.asset_key != asset_key {
            return Err(ResponseError::MixedAssets {
                first: asset_key.as_str().to_string(),
                other: obs.asset_key.as_str().to_string(),
            });
        }
    }
    for pair in observations.windows(2) {
        if pair[0].day == pair[1].day {
            return Err(ResponseError::DuplicateObservation {
                asset: asset_key.as_str().to_string(),
                day: pair[0].day,
            });
        }
    }

    let window_days = window.expected_session_count(session_calendar);
    let days_with_price = observations.iter().filter(|o| o.price.is_some()).count();
    let days_with_volume = observations.iter().filter(|o| o.volume.is_some()).count();

    let priced: Vec<Decimal> = observations.iter().filter_map(|o| o.price).collect();
    let (first_price, last_price, price_return) = if priced.len() >= 2 {
        let first = priced[0];
        let last = priced[priced.len() - 1];
        let ret = if first == Decimal::ZERO {
            None
        } else {
            Some((last / first) - Decimal::ONE)
        };
        (Some(first), Some(last), ret)
    } else if priced.len() == 1 {
        (Some(priced[0]), Some(priced[0]), None)
    } else {
        (None, None, None)
    };

    let window_vols: Vec<Decimal> = observations.iter().filter_map(|o| o.volume).collect();
    let baseline_vols: Vec<Decimal> = baseline.iter().filter_map(|o| o.volume).collect();
    let window_median_volume = median(&window_vols);
    let baseline_median_volume = median(&baseline_vols);
    let baseline_normalized_volume = match (window_median_volume, baseline_median_volume) {
        (Some(w), Some(b)) if b > Decimal::ZERO => Some(w / b),
        _ => None,
    };

    Ok(ResponseSeriesSummary {
        asset_key,
        window_name: window.name.clone(),
        observed_days: observations.len(),
        window_days,
        days_with_price,
        days_with_volume,
        price_return,
        first_price,
        last_price,
        baseline_normalized_volume,
        window_median_volume,
        baseline_median_volume,
        interpretation_boundary:
            "Market-response accounting only. Does not identify directional capital flow or migration.",
    })
}

fn median(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / Decimal::from(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn obs(day: &str, price: Option<&str>, volume: Option<&str>) -> ResponseObservation {
        ResponseObservation {
            asset_key: AssetKey::new("GC"),
            day: NaiveDate::from_str(day).unwrap(),
            price: price.map(|s| Decimal::from_str(s).unwrap()),
            volume: volume.map(|s| Decimal::from_str(s).unwrap()),
        }
    }

    fn window() -> EventWindow {
        EventWindow {
            name: "event".into(),
            start: NaiveDate::from_str("2026-06-12").unwrap(),
            end: NaiveDate::from_str("2026-06-14").unwrap(),
            applies_to: crate::event::default_window_applies_to(),
        }
    }

    #[test]
    fn price_return_and_volume_ratio() {
        let series = vec![
            obs("2026-06-12", Some("100"), Some("10")),
            obs("2026-06-13", Some("110"), Some("30")),
            obs("2026-06-14", Some("121"), Some("20")),
        ];
        let baseline = vec![
            obs("2026-01-01", Some("90"), Some("10")),
            obs("2026-01-02", Some("91"), Some("10")),
        ];
        let s = account_market_response(series, &window(), &baseline, SessionCalendar::Continuous)
            .unwrap();
        assert_eq!(s.price_return.unwrap(), Decimal::from_str("0.21").unwrap());
        assert_eq!(s.window_days, 3);
        assert_eq!(s.window_median_volume.unwrap(), Decimal::from(20));
        assert_eq!(s.baseline_median_volume.unwrap(), Decimal::from(10));
        assert_eq!(
            s.baseline_normalized_volume.unwrap(),
            Decimal::from_str("2").unwrap()
        );
    }

    #[test]
    fn exchange_sessions_denominator_skips_weekend() {
        // Fri 2026-06-12 .. Sun 2026-06-14 → weekdays = Fri only = 1 expected session.
        let series = vec![obs("2026-06-12", Some("100"), Some("10"))];
        let s = account_market_response(series, &window(), &[], SessionCalendar::ExchangeSessions)
            .unwrap();
        assert_eq!(s.window_days, 1);
        assert_eq!(s.observed_days, 1);
    }

    #[test]
    fn missing_prices_stay_none_not_zero() {
        let series = vec![
            obs("2026-06-12", None, Some("1")),
            obs("2026-06-13", None, Some("2")),
        ];
        let s =
            account_market_response(series, &window(), &[], SessionCalendar::Continuous).unwrap();
        assert!(s.price_return.is_none());
        assert!(s.first_price.is_none());
        assert!(s.baseline_normalized_volume.is_none());
    }

    #[test]
    fn rejects_duplicate_day() {
        let series = vec![
            obs("2026-06-12", Some("1"), None),
            obs("2026-06-12", Some("2"), None),
        ];
        let err = account_market_response(series, &window(), &[], SessionCalendar::Continuous)
            .unwrap_err();
        assert!(matches!(err, ResponseError::DuplicateObservation { .. }));
    }

    #[test]
    fn rejects_mixed_assets() {
        let mut series = vec![
            obs("2026-06-12", Some("1"), None),
            obs("2026-06-13", Some("2"), None),
        ];
        series[1].asset_key = AssetKey::new("OTHER");
        let err = account_market_response(series, &window(), &[], SessionCalendar::Continuous)
            .unwrap_err();
        assert!(matches!(err, ResponseError::MixedAssets { .. }));
    }
}
