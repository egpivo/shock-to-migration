//! Reusable market measurement primitives: shock score, horizon returns,
//! activity anomaly, reference-to-token response gap, divergence.
//!
//! This module is deliberately generic — no Gold/Oil (or any other
//! project-specific) logic, and no causal inference, difference-in-
//! differences, regression, VAR, or migration classification. It answers
//! "what did the series do" questions with explicit missingness semantics
//! (`None` for unknown, never a fabricated `0`), the same way `response.rs`
//! and `flow.rs` do.
//!
//! See `.local/docs/MEASUREMENT_CONTRACT.md` for the full formula reference.
//!
//! ## Submodules
//! - [`stats`]: generic decimal statistics (mean, population std, median,
//!   z-score) shared by everything else here.
//! - [`returns`]: daily simple returns from priced observations, plus the
//!   shared single-asset validation helpers (`MeasureError`).
//! - [`shock`]: [`ShockScore`] (event-day return z-scored against a
//!   baseline window) and the bundled [`shock::AssetShockReport`].
//! - [`horizon`]: [`horizon::HorizonReturns`], cumulative return N observed
//!   trading sessions after an event start.
//! - [`activity`]: [`activity::ActivityAnomaly`], ratio of window/baseline
//!   median volume.
//! - [`response_gap`]: [`response_gap::ResponseGapSummary`], a frozen
//!   reference return compared with the token's same-date return.
//! - [`divergence`]: [`divergence::DivergenceSummary`], `D_t = r_A - r_B` on
//!   matched trading days, z-scored against a baseline window.
//! - [`project_support`]: wires the above to `ProjectConfig` + the existing
//!   daily-response CSV ingest, for CLI use.
//! - [`format`]: terminal-friendly text summaries for CLI output.

pub mod activity;
pub mod divergence;
pub mod format;
pub mod horizon;
pub mod project_support;
pub mod response_gap;
pub mod returns;
pub mod shock;
pub mod stats;

pub use activity::{account_activity_anomaly, ActivityAnomaly};
pub use divergence::{account_divergence, load_divergence_summary, DivergenceSummary};
pub use format::{
    format_divergence_summary, format_response_gap_summary, format_shock_report_summary,
};
pub use horizon::{cumulative_return_from_event, HorizonReturn, HorizonReturns};
pub use project_support::MeasureProjectError;
pub use response_gap::{account_response_gap, load_response_gap_summary, ResponseGapSummary};
pub use returns::{daily_returns, DailyReturn, MeasureError};
pub use shock::{account_shock_score, load_asset_shock_report, AssetShockReport, ShockScore};
pub use stats::{
    compute_z_score, mean, median, population_std, ZScoreResult, ZScoreUnavailable,
    ADEQUATE_BASELINE_OBSERVATIONS, MIN_BASELINE_OBSERVATIONS,
};
