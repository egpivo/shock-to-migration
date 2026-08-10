//! Generic decimal statistics primitives shared by the `measure` module.
//!
//! No asset/window semantics live here — only arithmetic on `&[Decimal]`.

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Minimum number of baseline observations required before a z-score is
/// considered computable. Below this, variance is not meaningfully estimable.
pub const MIN_BASELINE_OBSERVATIONS: usize = 2;

/// Soft adequacy floor for an empirical baseline used as a z-score reference.
///
/// A z-score is still computed when `MIN_BASELINE_OBSERVATIONS ≤ n < this`,
/// but callers should flag `low_baseline` / emit a coverage gap: the number
/// is mathematically defined, not statistically trustworthy for descriptive
/// comparison against a thin empirical distribution.
pub const ADEQUATE_BASELINE_OBSERVATIONS: usize = 20;

pub fn sum(values: &[Decimal]) -> Decimal {
    values.iter().fold(Decimal::ZERO, |acc, v| acc + *v)
}

/// Arithmetic mean. `None` for an empty slice (never coerced to zero).
pub fn mean(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    Some(sum(values) / Decimal::from(values.len() as u64))
}

/// Population standard deviation (divisor `n`, not `n - 1`).
///
/// The baseline window is treated as the full reference population for the
/// window it covers, not a sample used to infer a broader population — so
/// population (not sample/Bessel-corrected) variance is used throughout
/// `measure`. `None` for an empty slice.
///
/// The variance is computed in exact `Decimal` arithmetic; the final square
/// root is taken via an IEEE-754 `f64` round-trip (`rust_decimal` has no
/// native decimal `sqrt` without the `maths` feature). Expect the result to
/// carry ordinary floating-point precision (~1e-15 relative), not exact
/// decimal precision.
pub fn population_std(values: &[Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    let m = mean(values)?;
    let sq_sum = values.iter().fold(Decimal::ZERO, |acc, v| {
        let d = *v - m;
        acc + d * d
    });
    let variance = sq_sum / Decimal::from(values.len() as u64);
    decimal_sqrt(variance)
}

/// Square root via `f64`, rounded to 12 decimal places to damp binary/decimal
/// round-trip noise. `None` for negative input (should not occur for a
/// variance) or if the `f64` round-trip fails.
pub fn decimal_sqrt(value: Decimal) -> Option<Decimal> {
    if value < Decimal::ZERO {
        return None;
    }
    if value == Decimal::ZERO {
        return Some(Decimal::ZERO);
    }
    let as_f64 = value.to_f64()?;
    if !as_f64.is_finite() {
        return None;
    }
    let root = as_f64.sqrt();
    Decimal::from_f64(root).map(|d| d.round_dp(12))
}

/// Standard median: for even `n`, the average of the two middle values (not
/// the upper of the two, and not the lower). `None` for an empty slice.
pub fn median(values: &[Decimal]) -> Option<Decimal> {
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

/// Why a z-score could not be computed. "Unknown" is distinct from "zero":
/// a zero-variance baseline is reported explicitly, not silently as `z = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum ZScoreUnavailable {
    /// The event-side value itself is missing (e.g. no priced observation on
    /// the focal day).
    NoEventValue,
    /// Fewer than [`MIN_BASELINE_OBSERVATIONS`] baseline points.
    InsufficientBaseline { have: usize, need: usize },
    /// Baseline has enough points but zero dispersion (division by zero
    /// would otherwise be silently avoided by returning `None`).
    ZeroBaselineVariance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZScoreResult {
    pub baseline_n: usize,
    pub baseline_mean: Option<Decimal>,
    pub baseline_std: Option<Decimal>,
    pub z_score: Option<Decimal>,
    /// True when a z-score was computed but `baseline_n` is below
    /// [`ADEQUATE_BASELINE_OBSERVATIONS`].
    pub low_baseline: bool,
    pub unavailable_reason: Option<ZScoreUnavailable>,
}

/// `z = (event_value - baseline_mean) / baseline_std`, computed against a
/// population baseline distribution. See module docs for edge-case ordering:
/// missing event value takes priority over an insufficient/zero-variance
/// baseline, since neither baseline defect explains the missing event value.
pub fn compute_z_score(event_value: Option<Decimal>, baseline_values: &[Decimal]) -> ZScoreResult {
    let baseline_n = baseline_values.len();
    let baseline_mean = mean(baseline_values);
    let baseline_std = population_std(baseline_values);

    let Some(event_value) = event_value else {
        return ZScoreResult {
            baseline_n,
            baseline_mean,
            baseline_std,
            z_score: None,
            low_baseline: false,
            unavailable_reason: Some(ZScoreUnavailable::NoEventValue),
        };
    };

    if baseline_n < MIN_BASELINE_OBSERVATIONS {
        return ZScoreResult {
            baseline_n,
            baseline_mean,
            baseline_std,
            z_score: None,
            low_baseline: false,
            unavailable_reason: Some(ZScoreUnavailable::InsufficientBaseline {
                have: baseline_n,
                need: MIN_BASELINE_OBSERVATIONS,
            }),
        };
    }

    let std = baseline_std.unwrap_or(Decimal::ZERO);
    if std == Decimal::ZERO {
        return ZScoreResult {
            baseline_n,
            baseline_mean,
            baseline_std,
            z_score: None,
            low_baseline: false,
            unavailable_reason: Some(ZScoreUnavailable::ZeroBaselineVariance),
        };
    }

    let z = (event_value - baseline_mean.unwrap_or(Decimal::ZERO)) / std;
    let low_baseline = baseline_n < ADEQUATE_BASELINE_OBSERVATIONS;
    ZScoreResult {
        baseline_n,
        baseline_mean,
        baseline_std,
        z_score: Some(z),
        low_baseline,
        unavailable_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn median_even_n_is_not_upper_median() {
        // Sorted: 1, 2, 3, 4 — standard median = 2.5; upper-median (wrong) = 3.
        let values = vec![d("4"), d("1"), d("3"), d("2")];
        let m = median(&values).unwrap();
        assert_eq!(m, d("2.5"));
        assert_ne!(m, d("3"), "must not use the upper of the two middle values");
    }

    #[test]
    fn median_odd_n() {
        let values = vec![d("5"), d("1"), d("3")];
        assert_eq!(median(&values).unwrap(), d("3"));
    }

    #[test]
    fn median_empty_is_none() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn population_std_known_fixture() {
        // mean = 0.02, deviations +/-0.01, population variance = 0.0001, std = 0.01.
        let values = vec![d("0.01"), d("0.03"), d("0.01"), d("0.03")];
        let std = population_std(&values).unwrap();
        assert_eq!(std, d("0.01"));
    }

    #[test]
    fn zero_variance_baseline_is_zero_variance_reason() {
        let values = vec![d("0.02"), d("0.02"), d("0.02")];
        let z = compute_z_score(Some(d("0.05")), &values);
        assert!(z.z_score.is_none());
        assert!(!z.low_baseline);
        assert_eq!(z.baseline_std, Some(Decimal::ZERO));
        assert_eq!(
            z.unavailable_reason,
            Some(ZScoreUnavailable::ZeroBaselineVariance)
        );
    }

    #[test]
    fn insufficient_baseline_reason() {
        let values = vec![d("0.01")];
        let z = compute_z_score(Some(d("0.05")), &values);
        assert!(z.z_score.is_none());
        assert!(!z.low_baseline);
        assert_eq!(
            z.unavailable_reason,
            Some(ZScoreUnavailable::InsufficientBaseline { have: 1, need: 2 })
        );
    }

    #[test]
    fn thin_but_computable_baseline_is_low_baseline() {
        let values = vec![d("0.01"), d("0.03")];
        let z = compute_z_score(Some(d("0.05")), &values);
        assert!(z.z_score.is_some());
        assert!(z.low_baseline);
        assert_eq!(z.baseline_n, 2);
        assert!(z.unavailable_reason.is_none());
    }

    #[test]
    fn missing_event_value_takes_priority_over_baseline_defects() {
        // Baseline is also insufficient here, but the reason must be NoEventValue.
        let z = compute_z_score(None, &[d("0.01")]);
        assert_eq!(z.unavailable_reason, Some(ZScoreUnavailable::NoEventValue));
        assert!(!z.low_baseline);
    }

    #[test]
    fn known_z_score_fixture() {
        let baseline = vec![d("0.01"), d("0.03"), d("0.01"), d("0.03")];
        let z = compute_z_score(Some(d("0.05")), &baseline);
        assert_eq!(z.baseline_mean, Some(d("0.02")));
        assert_eq!(z.baseline_std, Some(d("0.01")));
        assert_eq!(z.z_score, Some(d("3")));
        assert!(z.low_baseline); // n=4 < 20
    }
}
