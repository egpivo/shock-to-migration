//! Daily simple returns from a single asset's priced observations.
//!
//! Convention: **simple** returns, `r_t = p_t / p_{t-1} - 1`, matching the
//! `price_return` style already used by `market_response.v1`. Returns are
//! built from consecutive **priced** observations (days with `price = None`
//! are skipped, not zero-filled), so a return's `prev_day` is not
//! necessarily the calendar day before `day` — it is the previous day with
//! an observed price. This lets a single per-asset series feed both a
//! baseline window and an event window without needing the caller to
//! pre-split observations at window boundaries (which would silently drop
//! the return that spans the boundary).

use std::collections::HashSet;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::AssetKey;
use crate::response::ResponseObservation;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MeasureError {
    #[error("empty observation series for asset {0}")]
    EmptySeries(String),
    #[error("multiple asset_keys in one series (first={first}, also={other})")]
    MixedAssets { first: String, other: String },
    #[error("duplicate observation for asset {asset} on {day}")]
    DuplicateObservation { asset: String, day: NaiveDate },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyReturn {
    /// The day of `p_t` (the return is attributed to this day).
    pub day: NaiveDate,
    /// The day of `p_{t-1}` — the previous *priced* observation, which may
    /// be earlier than the calendar day before `day`.
    pub prev_day: NaiveDate,
    /// `p_t / p_{t-1} - 1`.
    pub value: Decimal,
}

/// Validates a (possibly unsorted) single-asset observation series: no mixed
/// `asset_key`s, no duplicate `day`s. Duplicates/mixed assets are hard
/// errors (reject, don't guess); an empty slice is not an error here and
/// yields `Ok(None)` since "no rows for this scope" is a legitimate,
/// non-exceptional state for callers like activity-anomaly medians.
pub(crate) fn validate_series(
    observations: &[ResponseObservation],
) -> Result<Option<AssetKey>, MeasureError> {
    if observations.is_empty() {
        return Ok(None);
    }
    let asset_key = observations[0].asset_key.clone();
    let mut seen_days: HashSet<NaiveDate> = HashSet::new();
    for obs in observations {
        if obs.asset_key != asset_key {
            return Err(MeasureError::MixedAssets {
                first: asset_key.as_str().to_string(),
                other: obs.asset_key.as_str().to_string(),
            });
        }
        if !seen_days.insert(obs.day) {
            return Err(MeasureError::DuplicateObservation {
                asset: asset_key.as_str().to_string(),
                day: obs.day,
            });
        }
    }
    Ok(Some(asset_key))
}

/// Sorts by day and validates as a single-asset, duplicate-free series.
/// Unlike [`validate_series`], an empty input **is** an error here: callers
/// of `sorted_validated` (returns, horizons) need at least one observation
/// to say anything at all.
pub(crate) fn sorted_validated(
    mut observations: Vec<ResponseObservation>,
) -> Result<(AssetKey, Vec<ResponseObservation>), MeasureError> {
    observations.sort_by_key(|o| o.day);
    match validate_series(&observations)? {
        Some(key) => Ok((key, observations)),
        None => Err(MeasureError::EmptySeries("<unknown>".into())),
    }
}

pub(crate) fn priced_only(sorted: &[ResponseObservation]) -> Vec<&ResponseObservation> {
    sorted.iter().filter(|o| o.price.is_some()).collect()
}

/// Pure transform: sorted single-asset observations -> daily simple returns.
/// A pair whose earlier price is zero is skipped (division undefined), not
/// treated as an error — mirrors `price_return`'s `None`-on-zero-first-price
/// behavior in `response.rs`.
pub(crate) fn returns_from_sorted(sorted: &[ResponseObservation]) -> Vec<DailyReturn> {
    let priced = priced_only(sorted);
    let mut out = Vec::with_capacity(priced.len());
    for pair in priced.windows(2) {
        let prev_price = pair[0].price.expect("priced_only guarantees Some");
        let cur_price = pair[1].price.expect("priced_only guarantees Some");
        if prev_price == Decimal::ZERO {
            continue;
        }
        out.push(DailyReturn {
            day: pair[1].day,
            prev_day: pair[0].day,
            value: cur_price / prev_price - Decimal::ONE,
        });
    }
    out
}

/// Daily simple returns for one asset's observation series (any order).
///
/// # Errors
/// Empty series, mixed `asset_key`s, or duplicate `(asset_key, day)` rows.
pub fn daily_returns(
    observations: Vec<ResponseObservation>,
) -> Result<Vec<DailyReturn>, MeasureError> {
    let (_, sorted) = sorted_validated(observations)?;
    Ok(returns_from_sorted(&sorted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn obs(day: &str, price: Option<&str>) -> ResponseObservation {
        ResponseObservation {
            asset_key: AssetKey::new("GC"),
            day: NaiveDate::from_str(day).unwrap(),
            price: price.map(|s| Decimal::from_str(s).unwrap()),
            volume: None,
        }
    }

    #[test]
    fn simple_returns_skip_missing_price_days() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-02", None),
            obs("2026-01-03", Some("110")),
        ];
        let returns = daily_returns(series).unwrap();
        assert_eq!(returns.len(), 1);
        assert_eq!(returns[0].day.to_string(), "2026-01-03");
        assert_eq!(returns[0].prev_day.to_string(), "2026-01-01");
        assert_eq!(returns[0].value, Decimal::from_str("0.1").unwrap());
    }

    #[test]
    fn zero_prev_price_pair_is_skipped_not_error() {
        let series = vec![obs("2026-01-01", Some("0")), obs("2026-01-02", Some("10"))];
        let returns = daily_returns(series).unwrap();
        assert!(returns.is_empty());
    }

    #[test]
    fn rejects_duplicate_day() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-01", Some("101")),
        ];
        let err = daily_returns(series).unwrap_err();
        assert!(matches!(err, MeasureError::DuplicateObservation { .. }));
    }

    #[test]
    fn rejects_mixed_assets() {
        let mut series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-02", Some("101")),
        ];
        series[1].asset_key = AssetKey::new("OTHER");
        let err = daily_returns(series).unwrap_err();
        assert!(matches!(err, MeasureError::MixedAssets { .. }));
    }

    #[test]
    fn rejects_empty_series() {
        let err = daily_returns(vec![]).unwrap_err();
        assert!(matches!(err, MeasureError::EmptySeries(_)));
    }

    #[test]
    fn unsorted_input_is_sorted_before_pairing() {
        let series = vec![
            obs("2026-01-03", Some("110")),
            obs("2026-01-01", Some("100")),
        ];
        let returns = daily_returns(series).unwrap();
        assert_eq!(returns.len(), 1);
        assert_eq!(returns[0].value, Decimal::from_str("0.1").unwrap());
    }
}
