//! Project-aware loaders that connect `measure`'s pure accounting functions
//! to `ProjectConfig` + the existing daily-response CSV ingest.
//!
//! This intentionally mirrors `report.rs`'s pattern (load -> filter -> call
//! pure accounting fn) rather than extending `respond`: `respond` reports
//! the declared `market_response.v1` section for *all* assets across *all*
//! response-tagged windows, while `measure` tools are single-asset (or
//! asset-pair), single-window-pair, on-demand queries driven by CLI flags.
//! Folding them into `respond`'s output would conflate "the declared
//! section" with "an ad hoc measurement", so they stay a separate surface.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::coverage::{AnalysisSection, CoverageGap, EvidenceBoundary, MissingKind};
use crate::event::{EventWindow, SessionCalendar};
use crate::identity::AssetKey;
use crate::ingest::daily_response::{load_daily_response, ResponseIngestError};
use crate::measure::returns::MeasureError;
use crate::measure::stats::ADEQUATE_BASELINE_OBSERVATIONS;
use crate::project::ProjectConfig;
use crate::response::ResponseObservation;

#[derive(Debug, Error)]
pub enum MeasureProjectError {
    #[error("ingest error: {0}")]
    Ingest(#[from] ResponseIngestError),
    #[error("project has no inputs.response declared")]
    NoResponseInput,
    #[error("no [response] baseline_window declared and no --baseline-window override given")]
    NoBaselineWindow,
    #[error("window '{0}' not found among declared project windows")]
    WindowNotFound(String),
    #[error(
        "no window with applies_to including response is available as a default event window (baseline window '{0}' excluded); pass --event-window explicitly"
    )]
    NoDefaultEventWindow(String),
    #[error("asset '{asset}' is not declared in project.toml")]
    UndeclaredAsset { asset: String },
    #[error("asset '{asset}' is declared but has no response rows in {path}")]
    NoObservationsForAsset { asset: String, path: String },
    #[error("measurement error: {0}")]
    Measure(#[from] MeasureError),
}

pub(crate) fn load_response_rows(
    cfg: &ProjectConfig,
) -> Result<Vec<ResponseObservation>, MeasureProjectError> {
    let rel = cfg
        .inputs
        .response
        .as_ref()
        .ok_or(MeasureProjectError::NoResponseInput)?;
    let path = cfg.root.join(rel);
    Ok(load_daily_response(&path)?)
}

pub(crate) fn observations_for_asset(
    rows: &[ResponseObservation],
    asset: &AssetKey,
) -> Vec<ResponseObservation> {
    rows.iter()
        .filter(|o| &o.asset_key == asset)
        .cloned()
        .collect()
}

pub(crate) fn require_asset_observations(
    cfg: &ProjectConfig,
    rows: &[ResponseObservation],
    asset: &AssetKey,
) -> Result<Vec<ResponseObservation>, MeasureProjectError> {
    if !cfg.assets.iter().any(|a| &a.key == asset) {
        return Err(MeasureProjectError::UndeclaredAsset {
            asset: asset.to_string(),
        });
    }
    let series = observations_for_asset(rows, asset);
    if series.is_empty() {
        return Err(MeasureProjectError::NoObservationsForAsset {
            asset: asset.to_string(),
            path: cfg
                .inputs
                .response
                .clone()
                .unwrap_or_else(|| "<none>".into()),
        });
    }
    Ok(series)
}

pub(crate) fn resolve_baseline_window<'a>(
    cfg: &'a ProjectConfig,
    override_name: Option<&str>,
) -> Result<&'a EventWindow, MeasureProjectError> {
    let name = match override_name {
        Some(n) => n.to_string(),
        None => cfg
            .response
            .as_ref()
            .ok_or(MeasureProjectError::NoBaselineWindow)?
            .baseline_window
            .clone(),
    };
    cfg.windows
        .iter()
        .find(|w| w.name == name)
        .ok_or(MeasureProjectError::WindowNotFound(name))
}

/// Default event window (when `--event-window` is omitted): the first
/// declared window (by declaration order) with `applies_to` including
/// `response`, excluding the baseline window itself.
pub(crate) fn resolve_event_window<'a>(
    cfg: &'a ProjectConfig,
    override_name: Option<&str>,
    baseline_name: &str,
) -> Result<&'a EventWindow, MeasureProjectError> {
    if let Some(n) = override_name {
        return cfg
            .windows
            .iter()
            .find(|w| w.name == n)
            .ok_or_else(|| MeasureProjectError::WindowNotFound(n.to_string()));
    }
    cfg.windows
        .iter()
        .find(|w| w.applies_to_response() && w.name != baseline_name)
        .ok_or_else(|| MeasureProjectError::NoDefaultEventWindow(baseline_name.to_string()))
}

fn session_calendar_for(cfg: &ProjectConfig, asset: &AssetKey) -> SessionCalendar {
    cfg.assets
        .iter()
        .find(|a| &a.key == asset)
        .map(|a| a.session_calendar)
        .unwrap_or_default()
}

fn session_label(calendar: SessionCalendar) -> &'static str {
    match calendar {
        SessionCalendar::Continuous => "calendar",
        SessionCalendar::ExchangeSessions => "weekday",
    }
}

/// Attach declared caveats + coverage for the windows this measure call uses,
/// plus an optional low-baseline adequacy gap.
pub(crate) fn build_measure_boundary(
    cfg: &ProjectConfig,
    assets: &[&AssetKey],
    windows: &[&EventWindow],
    series_by_asset: &[(&AssetKey, &[ResponseObservation])],
    low_baseline: bool,
    baseline_n: usize,
) -> EvidenceBoundary {
    let mut boundary = EvidenceBoundary::default();
    boundary.merge_declared(cfg.coverage_declared.clone());
    boundary.assume(
        "measure tools are descriptive accounting only; they do not infer causality or migration",
    );

    for asset in assets {
        let calendar = session_calendar_for(cfg, asset);
        let series = series_by_asset
            .iter()
            .find(|(k, _)| *k == *asset)
            .map(|(_, s)| *s)
            .unwrap_or(&[]);
        let observed: BTreeSet<_> = series.iter().map(|o| o.day).collect();

        for window in windows {
            let expected = window.expected_session_count(calendar);
            let missing = window.missing_sessions(calendar, &observed);
            if missing.is_empty() {
                continue;
            }
            boundary.push_detected(CoverageGap::detected(
                AnalysisSection::Measure,
                MissingKind::Other("response_coverage".into()),
                format!("{}@{}", asset, window.name),
                format!(
                    "{} of {} {} sessions missing in window '{}'",
                    missing.len(),
                    expected,
                    session_label(calendar),
                    window.name
                ),
            ));
        }
    }

    if low_baseline {
        boundary.push_detected(CoverageGap::detected(
            AnalysisSection::Measure,
            MissingKind::Other("low_baseline".into()),
            assets
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join("+"),
            format!(
                "baseline_n={baseline_n} is below adequacy floor {ADEQUATE_BASELINE_OBSERVATIONS}; z-score is computable but not a trustworthy empirical reference"
            ),
        ));
    }

    boundary
}
