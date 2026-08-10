//! Directional flow accounting.
//!
//! Gross A→B is never labeled migration. Reverse flow is first-class.
//! Missing denominators yield `None`, not zero ratios.
//!
//! Metric definition: `directional_flow.v2`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::EventWindow;
use crate::identity::AssetKey;

/// Non-negative quantity in an explicit unit. Construction requires a value;
/// there is no silent zero default used as "unknown".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Quantity(Decimal);

impl Quantity {
    pub fn new(value: Decimal) -> Result<Self, FlowError> {
        // Use `< 0`, not `is_sign_negative()`, so decimal negative-zero is accepted as zero.
        if value < Decimal::ZERO {
            return Err(FlowError::NegativeQuantity(value));
        }
        Ok(Self(value))
    }

    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    pub fn as_decimal(self) -> Decimal {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, FlowError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(FlowError::QuantityOverflow)
    }
}

impl std::fmt::Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowUnit {
    /// Native units of `asset` (typically the source leg of a conversion route).
    TokenNative {
        asset: AssetKey,
    },
    QuoteUsd,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionMethod {
    /// Swaps matched on the asset/mint pair (not necessarily issuer-operated).
    MintPairSwaps,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectionalFlowObservation {
    pub route_id: String,
    pub day: NaiveDate,
    pub gross_a_to_b: Quantity,
    pub gross_b_to_a: Quantity,
    pub unit: FlowUnit,
    pub attribution: AttributionMethod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CumulativePoint {
    pub day: NaiveDate,
    pub cumulative_net: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowSeriesSummary {
    pub route_id: String,
    pub window_name: String,
    pub unit: FlowUnit,
    pub attribution: AttributionMethod,
    /// Distinct days with observations inside the window.
    pub observed_days: usize,
    /// Inclusive calendar length of the named window.
    pub window_days: usize,
    pub gross_a_to_b_total: Quantity,
    pub gross_b_to_a_total: Quantity,
    /// `gross_a_to_b_total - gross_b_to_a_total` (signed).
    pub net_total: Decimal,
    pub cumulative_path: Vec<CumulativePoint>,
    /// Maximum of the observed cumulative path (not clamped at zero).
    pub peak_cumulative_net: Decimal,
    /// Minimum of the observed cumulative path.
    pub trough_cumulative_net: Decimal,
    /// Count of cumulative points strictly below zero.
    pub days_cumulative_negative: u32,
    /// Reverse gross per unit of forward gross: `B→A / A→B` when A→B > 0; else `None`.
    /// Values > 1 mean reverse-dominant traffic, not a percentage capped at 100%.
    pub reversal_ratio: Option<Decimal>,
    /// Net over a frozen denominator when available; `None` if denominator missing.
    pub net_over_denominator: Option<Decimal>,
    /// Count of flips between consecutive non-zero cumulative signs.
    pub sign_change_days: u32,
    /// Explicit note: this summary is accounting, not a migration verdict.
    pub interpretation_boundary: &'static str,
}

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("quantity must be non-negative, got {0}")]
    NegativeQuantity(Decimal),
    #[error("quantity overflow")]
    QuantityOverflow,
    #[error("empty observation series for route {0}")]
    EmptySeries(String),
    #[error("multiple route_ids in one series (first={first}, also={other})")]
    MixedRouteIds { first: String, other: String },
    #[error("duplicate observation for route {route_id} on {day}")]
    DuplicateObservation { route_id: String, day: NaiveDate },
    #[error("mixed units in series for route {0}")]
    MixedUnits(String),
    #[error("mixed attribution methods in series for route {0}")]
    MixedAttribution(String),
    #[error("denominator must be strictly positive, got {0}")]
    NonPositiveDenominator(Decimal),
}

/// Deterministic directional-flow accounting for one route inside one named window.
///
/// Observations must already be filtered to the window. Duplicate `(route_id, day)`
/// rows are rejected. Does **not** classify results as migration.
pub fn account_directional_flows(
    mut observations: Vec<DirectionalFlowObservation>,
    window: &EventWindow,
    denominator: Option<Decimal>,
) -> Result<FlowSeriesSummary, FlowError> {
    if observations.is_empty() {
        return Err(FlowError::EmptySeries("<unknown>".into()));
    }

    observations.sort_by_key(|o| o.day);

    let route_id = observations[0].route_id.clone();
    let unit = observations[0].unit.clone();
    let attribution = observations[0].attribution.clone();

    for obs in &observations {
        if obs.route_id != route_id {
            return Err(FlowError::MixedRouteIds {
                first: route_id,
                other: obs.route_id.clone(),
            });
        }
        if obs.unit != unit {
            return Err(FlowError::MixedUnits(route_id));
        }
        if obs.attribution != attribution {
            return Err(FlowError::MixedAttribution(route_id));
        }
    }

    for pair in observations.windows(2) {
        if pair[0].day == pair[1].day {
            return Err(FlowError::DuplicateObservation {
                route_id,
                day: pair[0].day,
            });
        }
    }

    if let Some(d) = denominator {
        if d <= Decimal::ZERO {
            return Err(FlowError::NonPositiveDenominator(d));
        }
    }

    let window_days = inclusive_day_count(window);

    let mut gross_a_to_b_total = Quantity::zero();
    let mut gross_b_to_a_total = Quantity::zero();
    let mut cumulative_path = Vec::with_capacity(observations.len());
    let mut sign_change_days = 0u32;
    let mut prev_sign: Option<i8> = None;
    let mut days_cumulative_negative = 0u32;

    // Initialize peak/trough from the first day's cumulative (series is non-empty).
    let first = &observations[0];
    gross_a_to_b_total = gross_a_to_b_total.checked_add(first.gross_a_to_b)?;
    gross_b_to_a_total = gross_b_to_a_total.checked_add(first.gross_b_to_a)?;
    let mut cumulative = first.gross_a_to_b.as_decimal() - first.gross_b_to_a.as_decimal();
    let mut peak = cumulative;
    let mut trough = cumulative;
    if cumulative < Decimal::ZERO {
        days_cumulative_negative += 1;
    }
    let first_sign = decimal_sign(cumulative);
    if first_sign != 0 {
        prev_sign = Some(first_sign);
    }
    cumulative_path.push(CumulativePoint {
        day: first.day,
        cumulative_net: cumulative,
    });

    for obs in observations.iter().skip(1) {
        gross_a_to_b_total = gross_a_to_b_total.checked_add(obs.gross_a_to_b)?;
        gross_b_to_a_total = gross_b_to_a_total.checked_add(obs.gross_b_to_a)?;
        let day_net = obs.gross_a_to_b.as_decimal() - obs.gross_b_to_a.as_decimal();
        cumulative += day_net;

        peak = peak.max(cumulative);
        trough = trough.min(cumulative);

        if cumulative < Decimal::ZERO {
            days_cumulative_negative += 1;
        }

        let sign = decimal_sign(cumulative);
        if let Some(prev) = prev_sign {
            if sign != 0 && prev != 0 && sign != prev {
                sign_change_days += 1;
            }
        }
        if sign != 0 {
            prev_sign = Some(sign);
        }
        cumulative_path.push(CumulativePoint {
            day: obs.day,
            cumulative_net: cumulative,
        });
    }

    let net_total = gross_a_to_b_total.as_decimal() - gross_b_to_a_total.as_decimal();

    let reversal_ratio = if gross_a_to_b_total.as_decimal() > Decimal::ZERO {
        Some(gross_b_to_a_total.as_decimal() / gross_a_to_b_total.as_decimal())
    } else {
        None
    };

    let net_over_denominator = denominator.map(|d| net_total / d);

    Ok(FlowSeriesSummary {
        route_id,
        window_name: window.name.clone(),
        unit,
        attribution,
        observed_days: observations.len(),
        window_days,
        gross_a_to_b_total,
        gross_b_to_a_total,
        net_total,
        cumulative_path,
        peak_cumulative_net: peak,
        trough_cumulative_net: trough,
        days_cumulative_negative,
        reversal_ratio,
        net_over_denominator,
        sign_change_days,
        interpretation_boundary:
            "Flow accounting only. Gross and net flows are not migration labels.",
    })
}

pub fn inclusive_day_count(window: &EventWindow) -> usize {
    (window.end - window.start).num_days() as usize + 1
}

fn decimal_sign(value: Decimal) -> i8 {
    if value > Decimal::ZERO {
        1
    } else if value < Decimal::ZERO {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn q(s: &str) -> Quantity {
        Quantity::new(Decimal::from_str(s).unwrap()).unwrap()
    }

    fn window() -> EventWindow {
        EventWindow {
            name: "post_event".into(),
            start: NaiveDate::from_str("2026-06-12").unwrap(),
            end: NaiveDate::from_str("2026-06-18").unwrap(),
        }
    }

    fn obs(day: &str, a_to_b: &str, b_to_a: &str) -> DirectionalFlowObservation {
        DirectionalFlowObservation {
            route_id: "a_b".into(),
            day: NaiveDate::from_str(day).unwrap(),
            gross_a_to_b: q(a_to_b),
            gross_b_to_a: q(b_to_a),
            unit: FlowUnit::TokenNative {
                asset: AssetKey::new("A"),
            },
            attribution: AttributionMethod::Fixture,
        }
    }

    #[test]
    fn accounts_gross_net_and_reversal() {
        let series = vec![
            obs("2026-06-12", "100", "20"),
            obs("2026-06-13", "50", "40"),
            obs("2026-06-14", "10", "30"),
        ];
        let summary =
            account_directional_flows(series, &window(), Some(Decimal::from(1000))).unwrap();
        assert_eq!(summary.window_name, "post_event");
        assert_eq!(summary.window_days, 7);
        assert_eq!(summary.observed_days, 3);
        assert_eq!(summary.gross_a_to_b_total.as_decimal(), Decimal::from(160));
        assert_eq!(summary.gross_b_to_a_total.as_decimal(), Decimal::from(90));
        assert_eq!(summary.net_total, Decimal::from(70));
        assert_eq!(
            summary.reversal_ratio.unwrap(),
            Decimal::from(90) / Decimal::from(160)
        );
        assert_eq!(
            summary.net_over_denominator.unwrap(),
            Decimal::from(70) / Decimal::from(1000)
        );
        assert_eq!(summary.peak_cumulative_net, Decimal::from(90)); // 80 then 90 then 70
        assert_eq!(summary.trough_cumulative_net, Decimal::from(70));
        assert_eq!(summary.days_cumulative_negative, 0);
        assert!(summary.interpretation_boundary.contains("not migration"));
    }

    #[test]
    fn missing_denominator_stays_none() {
        let series = vec![obs("2026-06-12", "10", "1")];
        let summary = account_directional_flows(series, &window(), None).unwrap();
        assert!(summary.net_over_denominator.is_none());
    }

    #[test]
    fn detects_cumulative_sign_changes() {
        let series = vec![
            obs("2026-06-12", "10", "0"),
            obs("2026-06-13", "0", "15"),
            obs("2026-06-14", "20", "0"),
        ];
        let summary = account_directional_flows(series, &window(), None).unwrap();
        assert_eq!(summary.sign_change_days, 2);
        assert_eq!(summary.net_total, Decimal::from(15));
        assert_eq!(summary.days_cumulative_negative, 1);
        assert_eq!(summary.trough_cumulative_net, Decimal::from(-5));
    }

    #[test]
    fn rejects_negative_quantity() {
        assert!(Quantity::new(Decimal::from(-1)).is_err());
    }

    #[test]
    fn accepts_decimal_negative_zero_as_zero() {
        let neg_zero = Decimal::from_str("-0").unwrap();
        let q = Quantity::new(neg_zero).unwrap();
        assert_eq!(q.as_decimal(), Decimal::ZERO);
    }

    #[test]
    fn rejects_non_positive_denominator() {
        let series = vec![obs("2026-06-12", "1", "0")];
        let err = account_directional_flows(series, &window(), Some(Decimal::ZERO)).unwrap_err();
        assert!(matches!(err, FlowError::NonPositiveDenominator(_)));
    }

    #[test]
    fn rejects_duplicate_day() {
        let series = vec![
            obs("2026-06-12", "100", "20"),
            obs("2026-06-12", "100", "20"),
        ];
        let err = account_directional_flows(series, &window(), None).unwrap_err();
        assert!(matches!(
            err,
            FlowError::DuplicateObservation { day, .. } if day == NaiveDate::from_str("2026-06-12").unwrap()
        ));
    }

    #[test]
    fn all_negative_path_peak_is_first_cumulative() {
        // day nets: -40, -20, -15 → cum -40, -60, -75
        let series = vec![
            obs("2026-06-12", "0", "40"),
            obs("2026-06-13", "0", "20"),
            obs("2026-06-14", "0", "15"),
        ];
        let summary = account_directional_flows(series, &window(), None).unwrap();
        assert_eq!(summary.net_total, Decimal::from(-75));
        assert_eq!(summary.peak_cumulative_net, Decimal::from(-40));
        assert_eq!(summary.trough_cumulative_net, Decimal::from(-75));
        assert_eq!(summary.days_cumulative_negative, 3);
        assert!(summary.reversal_ratio.is_none()); // forward gross is zero
    }

    #[test]
    fn reverse_only_series_reversal_ratio_is_none() {
        let series = vec![obs("2026-06-12", "0", "10")];
        let summary = account_directional_flows(series, &window(), None).unwrap();
        assert!(summary.reversal_ratio.is_none());
        assert_eq!(summary.net_total, Decimal::from(-10));
    }
}
