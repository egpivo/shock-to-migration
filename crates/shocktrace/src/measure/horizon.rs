//! Cumulative simple return from an anchor price to N observed trading
//! sessions after the event start.
//!
//! **Convention:** "N sessions after event start" counts observed *priced*
//! observations, not calendar days — a missing horizon (not enough later
//! priced observations) is `None`, never approximated by the nearest
//! available day and never coerced to `0`.
//!
//! Anchor price = the last priced observation on or before `event_start`
//! (not necessarily exactly on `event_start` — e.g. a holiday on the event
//! start date still anchors to the last known pre-event price). If there is
//! no priced observation on or before `event_start`, every horizon is
//! `None`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::identity::AssetKey;
use crate::measure::returns::{priced_only, sorted_validated, MeasureError};
use crate::response::ResponseObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonReturn {
    pub horizon_sessions: usize,
    /// Day of the priced observation `horizon_sessions` sessions after the
    /// anchor. `None` if that many later priced sessions are not observed.
    pub day: Option<NaiveDate>,
    pub cumulative_return: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonReturns {
    pub asset_key: AssetKey,
    pub event_start: NaiveDate,
    pub anchor_day: Option<NaiveDate>,
    pub anchor_price: Option<Decimal>,
    pub horizons: Vec<HorizonReturn>,
}

/// # Errors
/// Empty series, mixed `asset_key`s, or duplicate `(asset_key, day)` rows.
pub fn cumulative_return_from_event(
    observations: Vec<ResponseObservation>,
    event_start: NaiveDate,
    horizon_sessions: &[usize],
) -> Result<HorizonReturns, MeasureError> {
    let (asset_key, sorted) = sorted_validated(observations)?;
    let priced = priced_only(&sorted);

    let anchor_idx = priced.iter().rposition(|o| o.day <= event_start);
    let anchor_day = anchor_idx.map(|i| priced[i].day);
    let anchor_price = anchor_idx.and_then(|i| priced[i].price);

    let horizons = horizon_sessions
        .iter()
        .map(|&h| match (anchor_idx, anchor_price) {
            (Some(i), Some(anchor_p)) => {
                let target = i + h;
                match priced.get(target).and_then(|o| o.price.map(|p| (o.day, p))) {
                    Some((day, target_price)) if anchor_p != Decimal::ZERO => HorizonReturn {
                        horizon_sessions: h,
                        day: Some(day),
                        cumulative_return: Some(target_price / anchor_p - Decimal::ONE),
                    },
                    Some((day, _)) => HorizonReturn {
                        horizon_sessions: h,
                        day: Some(day),
                        cumulative_return: None,
                    },
                    None => HorizonReturn {
                        horizon_sessions: h,
                        day: None,
                        cumulative_return: None,
                    },
                }
            }
            _ => HorizonReturn {
                horizon_sessions: h,
                day: None,
                cumulative_return: None,
            },
        })
        .collect();

    Ok(HorizonReturns {
        asset_key,
        event_start,
        anchor_day,
        anchor_price,
        horizons,
    })
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

    fn d(day: &str) -> NaiveDate {
        NaiveDate::from_str(day).unwrap()
    }

    #[test]
    fn horizons_use_observed_sessions_not_calendar_days() {
        let series = vec![
            obs("2026-01-01", Some("100")), // anchor (event_start)
            obs("2026-01-02", None),        // missing day, not counted as a session
            obs("2026-01-03", Some("110")), // 1st observed session after anchor
            obs("2026-01-06", Some("121")), // 2nd observed session
        ];
        let result = cumulative_return_from_event(series, d("2026-01-01"), &[1, 2, 3]).unwrap();
        assert_eq!(result.anchor_day, Some(d("2026-01-01")));
        assert_eq!(result.horizons[0].day, Some(d("2026-01-03")));
        assert_eq!(
            result.horizons[0].cumulative_return,
            Some(Decimal::from_str("0.1").unwrap())
        );
        assert_eq!(result.horizons[1].day, Some(d("2026-01-06")));
        assert_eq!(
            result.horizons[1].cumulative_return,
            Some(Decimal::from_str("0.21").unwrap())
        );
        assert_eq!(result.horizons[2].day, None);
        assert!(result.horizons[2].cumulative_return.is_none());
    }

    #[test]
    fn anchor_falls_back_to_last_priced_day_on_or_before_event_start() {
        let series = vec![
            obs("2026-01-01", Some("100")), // event start has no price (holiday)...
            obs("2026-01-03", Some("110")),
        ];
        // event_start is 2026-01-02, a day with no observation at all.
        let result = cumulative_return_from_event(series, d("2026-01-02"), &[1]).unwrap();
        assert_eq!(result.anchor_day, Some(d("2026-01-01")));
        assert_eq!(result.horizons[0].day, Some(d("2026-01-03")));
        assert_eq!(
            result.horizons[0].cumulative_return,
            Some(Decimal::from_str("0.1").unwrap())
        );
    }

    #[test]
    fn no_anchor_yields_all_horizons_none() {
        let series = vec![obs("2026-06-01", Some("100"))];
        let result = cumulative_return_from_event(series, d("2026-01-01"), &[1, 3, 5, 20]).unwrap();
        assert!(result.anchor_day.is_none());
        assert!(result
            .horizons
            .iter()
            .all(|h| h.cumulative_return.is_none()));
        assert!(result.horizons.iter().all(|h| h.day.is_none()));
    }

    #[test]
    fn missing_horizon_is_none_not_zero() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-02", Some("101")),
        ];
        let result = cumulative_return_from_event(series, d("2026-01-01"), &[1, 5, 20]).unwrap();
        assert!(result.horizons[0].cumulative_return.is_some());
        assert!(result.horizons[1].cumulative_return.is_none());
        assert!(result.horizons[2].cumulative_return.is_none());
    }

    #[test]
    fn rejects_duplicate_day() {
        let series = vec![
            obs("2026-01-01", Some("100")),
            obs("2026-01-01", Some("101")),
        ];
        let err = cumulative_return_from_event(series, d("2026-01-01"), &[1]).unwrap_err();
        assert!(matches!(err, MeasureError::DuplicateObservation { .. }));
    }
}
