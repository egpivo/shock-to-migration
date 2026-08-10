//! Daily simple returns from a single asset's priced observations.
//!
//! Convention: **simple** returns, `r_t = p_t / p_{t-1} - 1`, matching the
//! `price_return` style already used by `market_response.v1`. Returns are
//! built only when both the current calendar day and the immediately
//! preceding calendar day have prices. A missing row or `price = None`
//! breaks the chain: the next observed price does not become a multi-day
//! return mislabeled as daily. The transform runs before window filtering,
//! so a valid one-day return that crosses a window boundary is retained and
//! attributed to its later day.

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
    /// The day of `p_{t-1}` — exactly one calendar day before `day`.
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
/// A missing calendar day, a missing price on either side, or a zero earlier
/// price breaks the return chain. These cases are skipped rather than filled
/// or coerced — mirrors `price_return`'s `None`-on-zero-first-price behavior
/// in `response.rs`.
pub(crate) fn returns_from_sorted(sorted: &[ResponseObservation]) -> Vec<DailyReturn> {
    let mut out = Vec::with_capacity(sorted.len());
    for pair in sorted.windows(2) {
        if pair[1].day.signed_duration_since(pair[0].day).num_days() != 1 {
            continue;
        }
        let (Some(prev_price), Some(cur_price)) = (pair[0].price, pair[1].price) else {
            continue;
        };
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
    fn missing_price_breaks_daily_return_chain() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-02", None),
            obs("2026-01-03", Some("110")),
        ];
        let returns = daily_returns(series).unwrap();
        assert!(returns.is_empty());
    }

    #[test]
    fn absent_calendar_day_breaks_daily_return_chain() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-03", Some("110")),
        ];
        let returns = daily_returns(series).unwrap();
        assert!(returns.is_empty());
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
            obs("2026-01-02", Some("110")),
            obs("2026-01-01", Some("100")),
        ];
        let returns = daily_returns(series).unwrap();
        assert_eq!(returns.len(), 1);
        assert_eq!(returns[0].value, Decimal::from_str("0.1").unwrap());
    }
}
