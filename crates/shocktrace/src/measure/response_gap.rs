//! Same-date reference-to-token response gaps.
//!
//! `gap = token_return - reference_return`. This is descriptive accounting:
//! the reference and token may use different market cutoffs, so the result
//! is not a synchronized hedge error, beta, or causal transmission estimate.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::coverage::{AnalysisSection, CoverageGap, EvidenceBoundary, MissingKind};
use crate::identity::AssetKey;
use crate::ingest::reference_returns::ReferenceReturnObservation;
use crate::measure::project_support::{
    build_measure_boundary, load_reference_rows, load_response_rows, require_asset_observations,
    resolve_baseline_window, resolve_event_window, MeasureProjectError,
};
use crate::measure::returns::daily_returns;
use crate::project::ProjectConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResponseGapSummary {
    pub asset_key: AssetKey,
    pub reference_key: String,
    pub event_day: NaiveDate,
    pub reference_return: Decimal,
    pub token_return: Option<Decimal>,
    /// `token_return - reference_return`.
    pub response_gap: Option<Decimal>,
    /// `Some(true)` only when both non-zero returns have the same sign.
    pub direction_match: Option<bool>,
    pub source_label: String,
    pub source_url: String,
    pub reference_cutoff: String,
    pub interpretation_boundary: &'static str,
    pub boundary: EvidenceBoundary,
}

const RESPONSE_GAP_INTERPRETATION_BOUNDARY: &str = "Response gap is the token's same-date daily return minus a frozen source-reported reference return. It is not a synchronized intraday comparison, beta, tracking-error model, pass-through coefficient, or causal transmission estimate.";

pub fn account_response_gap(
    reference: ReferenceReturnObservation,
    token_return: Option<Decimal>,
) -> ResponseGapSummary {
    let response_gap = token_return.map(|value| value - reference.reference_return);
    let direction_match = token_return.and_then(|value| {
        if value == Decimal::ZERO || reference.reference_return == Decimal::ZERO {
            None
        } else {
            Some(
                (value > Decimal::ZERO && reference.reference_return > Decimal::ZERO)
                    || (value < Decimal::ZERO && reference.reference_return < Decimal::ZERO),
            )
        }
    });

    ResponseGapSummary {
        asset_key: reference.asset_key,
        reference_key: reference.reference_key,
        event_day: reference.day,
        reference_return: reference.reference_return,
        token_return,
        response_gap,
        direction_match,
        source_label: reference.source_label,
        source_url: reference.source_url,
        reference_cutoff: reference.cutoff,
        interpretation_boundary: RESPONSE_GAP_INTERPRETATION_BOUNDARY,
        boundary: EvidenceBoundary::default(),
    }
}

pub fn load_response_gap_summary(
    cfg: &ProjectConfig,
    asset: &AssetKey,
    reference_key: &str,
    event_window_override: Option<&str>,
) -> Result<ResponseGapSummary, MeasureProjectError> {
    let baseline = resolve_baseline_window(cfg, None)?;
    let event_window = resolve_event_window(cfg, event_window_override, &baseline.name)?;
    let references = load_reference_rows(cfg)?;
    let mut candidates = references.into_iter().filter(|row| {
        &row.asset_key == asset
            && row.reference_key == reference_key
            && event_window.contains(row.day)
    });
    let reference = candidates
        .next()
        .ok_or_else(|| MeasureProjectError::ReferenceNotFound {
            reference: reference_key.to_string(),
            asset: asset.to_string(),
        })?;
    if candidates.next().is_some() {
        return Err(MeasureProjectError::AmbiguousReference {
            reference: reference_key.to_string(),
            asset: asset.to_string(),
        });
    }

    let rows = load_response_rows(cfg)?;
    let series = require_asset_observations(cfg, &rows, asset)?;
    let token_return = daily_returns(series.clone())?
        .into_iter()
        .find(|row| row.day == reference.day)
        .map(|row| row.value);

    let mut summary = account_response_gap(reference, token_return);
    summary.boundary = build_measure_boundary(
        cfg,
        &[asset],
        &[event_window],
        &[(asset, series.as_slice())],
        false,
        0,
    );
    summary.boundary.assume(format!(
        "reference '{}' uses {}; comparison is by observation date, not a synchronized intraday cutoff",
        summary.reference_key, summary.reference_cutoff
    ));
    if summary.token_return.is_none() {
        summary.boundary.push_detected(CoverageGap::detected(
            AnalysisSection::Measure,
            MissingKind::Price,
            format!("{}@{}", asset, summary.event_day),
            "no adjacent-calendar-day token return is available on the reference day",
        ));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn reference(value: &str) -> ReferenceReturnObservation {
        ReferenceReturnObservation {
            reference_key: "REF".into(),
            asset_key: AssetKey::new("TOKEN"),
            day: NaiveDate::from_str("2026-07-08").unwrap(),
            reference_return: Decimal::from_str(value).unwrap(),
            source_label: "fixture".into(),
            source_url: "https://example.test".into(),
            cutoff: "fixture close".into(),
        }
    }

    #[test]
    fn gap_is_token_minus_reference() {
        let summary = account_response_gap(
            reference("0.044"),
            Some(Decimal::from_str("0.0389").unwrap()),
        );
        assert_eq!(
            summary.response_gap,
            Some(Decimal::from_str("-0.0051").unwrap())
        );
        assert_eq!(summary.direction_match, Some(true));
    }

    #[test]
    fn missing_token_return_stays_missing() {
        let summary = account_response_gap(reference("-0.0052"), None);
        assert!(summary.response_gap.is_none());
        assert!(summary.direction_match.is_none());
    }
}
