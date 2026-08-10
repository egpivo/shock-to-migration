//! Activity anomaly: ratio of window median volume to baseline median volume.
//!
//! Uses the standard statistical median (even-`n` -> average of the two
//! middle values), matching the convention already used by
//! `market_response.v1`'s `baseline_normalized_volume`. This is a
//! standalone, reusable primitive — it does not read `ResponseConfig` or a
//! project's declared baseline window, unlike the `response` module's
//! built-in ratio, which is wired into the report pipeline.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::identity::AssetKey;
use crate::measure::returns::{validate_series, MeasureError};
use crate::measure::stats::median;
use crate::response::ResponseObservation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityAnomaly {
    pub asset_key: AssetKey,
    pub window_name: String,
    pub baseline_window_name: String,
    pub window_median_volume: Option<Decimal>,
    pub baseline_median_volume: Option<Decimal>,
    /// `window_median_volume / baseline_median_volume`. `None` if either
    /// median is unavailable (no volumes) or the baseline median is zero.
    pub ratio: Option<Decimal>,
    pub interpretation_boundary: &'static str,
}

const ACTIVITY_INTERPRETATION_BOUNDARY: &str = "Activity anomaly is a ratio of standard medians (even-N: average of the two middle values, not the upper one). It does not identify directional flow, trading intent, or migration.";

/// # Errors
/// Mixed `asset_key`s or duplicate `(asset_key, day)` rows within either
/// slice. Empty slices are not errors — they simply yield `None` medians.
pub fn account_activity_anomaly(
    asset_key: AssetKey,
    window_name: impl Into<String>,
    baseline_window_name: impl Into<String>,
    window_observations: Vec<ResponseObservation>,
    baseline_observations: Vec<ResponseObservation>,
) -> Result<ActivityAnomaly, MeasureError> {
    validate_series(&window_observations)?;
    validate_series(&baseline_observations)?;

    let window_vols: Vec<Decimal> = window_observations
        .iter()
        .filter_map(|o| o.volume)
        .collect();
    let baseline_vols: Vec<Decimal> = baseline_observations
        .iter()
        .filter_map(|o| o.volume)
        .collect();

    let window_median_volume = median(&window_vols);
    let baseline_median_volume = median(&baseline_vols);
    let ratio = match (window_median_volume, baseline_median_volume) {
        (Some(w), Some(b)) if b != Decimal::ZERO => Some(w / b),
        _ => None,
    };

    Ok(ActivityAnomaly {
        asset_key,
        window_name: window_name.into(),
        baseline_window_name: baseline_window_name.into(),
        window_median_volume,
        baseline_median_volume,
        ratio,
        interpretation_boundary: ACTIVITY_INTERPRETATION_BOUNDARY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::str::FromStr;

    fn obs(day: &str, volume: Option<&str>) -> ResponseObservation {
        ResponseObservation {
            asset_key: AssetKey::new("GC"),
            day: NaiveDate::from_str(day).unwrap(),
            price: None,
            volume: volume.map(|s| Decimal::from_str(s).unwrap()),
        }
    }

    #[test]
    fn even_n_uses_standard_median_not_upper_median() {
        // Window volumes sorted: 10, 20, 30, 40 -> standard median = 25; upper-median (wrong) = 30.
        let window = vec![
            obs("2026-01-01", Some("40")),
            obs("2026-01-02", Some("10")),
            obs("2026-01-03", Some("30")),
            obs("2026-01-04", Some("20")),
        ];
        let baseline = vec![obs("2025-01-01", Some("5")), obs("2025-01-02", Some("5"))];
        let a =
            account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", window, baseline)
                .unwrap();
        assert_eq!(
            a.window_median_volume,
            Some(Decimal::from_str("25").unwrap())
        );
        assert_ne!(
            a.window_median_volume,
            Some(Decimal::from_str("30").unwrap())
        );
    }

    #[test]
    fn ratio_of_medians() {
        let window = vec![obs("2026-01-01", Some("20")), obs("2026-01-02", Some("30"))];
        let baseline = vec![obs("2025-01-01", Some("5")), obs("2025-01-02", Some("15"))];
        let a =
            account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", window, baseline)
                .unwrap();
        assert_eq!(
            a.window_median_volume,
            Some(Decimal::from_str("25").unwrap())
        );
        assert_eq!(
            a.baseline_median_volume,
            Some(Decimal::from_str("10").unwrap())
        );
        assert_eq!(a.ratio, Some(Decimal::from_str("2.5").unwrap()));
    }

    #[test]
    fn missing_volume_is_none_not_zero() {
        let window = vec![obs("2026-01-01", None), obs("2026-01-02", None)];
        let baseline = vec![obs("2025-01-01", Some("5"))];
        let a =
            account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", window, baseline)
                .unwrap();
        assert!(a.window_median_volume.is_none());
        assert!(a.ratio.is_none());
    }

    #[test]
    fn zero_baseline_median_yields_none_ratio() {
        let window = vec![obs("2026-01-01", Some("10"))];
        let baseline = vec![obs("2025-01-01", Some("0")), obs("2025-01-02", Some("0"))];
        let a =
            account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", window, baseline)
                .unwrap();
        assert_eq!(a.baseline_median_volume, Some(Decimal::ZERO));
        assert!(a.ratio.is_none());
    }

    #[test]
    fn rejects_duplicate_day() {
        let window = vec![obs("2026-01-01", Some("10")), obs("2026-01-01", Some("20"))];
        let err =
            account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", window, vec![])
                .unwrap_err();
        assert!(matches!(err, MeasureError::DuplicateObservation { .. }));
    }

    #[test]
    fn empty_slices_are_not_errors() {
        let a = account_activity_anomaly(AssetKey::new("GC"), "event", "baseline", vec![], vec![])
            .unwrap();
        assert!(a.window_median_volume.is_none());
        assert!(a.baseline_median_volume.is_none());
        assert!(a.ratio.is_none());
    }
}
