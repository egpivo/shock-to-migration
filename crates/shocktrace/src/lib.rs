//! Shocktrace: deterministic measurement of market responses and directional flows.
//!
//! This crate measures empirical objects. It does **not** infer migration from
//! coincident activity changes (`A↓ + B↑ ⇏ capital moved A→B`).

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod control;
pub mod coverage;
pub mod event;
pub mod evidence;
pub mod flow;
pub mod identity;
pub mod ingest;
pub mod measure;
pub mod observe;
pub mod project;
pub mod provenance;
pub mod report;
pub mod response;
pub mod route;

pub use control::{ControlAsset, ControlRelation};
pub use coverage::{
    format_evidence_boundary, AnalysisSection, CoverageGap, EvidenceBoundary, GapSource,
    MissingKind,
};
pub use event::{default_window_applies_to, Event, EventWindow, SessionCalendar, WindowUse};
pub use evidence::EvidenceSection;
pub use flow::{
    account_directional_flows, AttributionMethod, DirectionalFlowObservation, FlowSeriesSummary,
    FlowUnit, Quantity,
};
pub use identity::{
    AssetId, AssetKey, AssetLocator, CanonicalAsset, ChainId, ProductKind, VenueId,
};
pub use measure::{
    account_activity_anomaly, account_divergence, account_passthrough, account_shock_score,
    cumulative_return_from_event, daily_returns, format_divergence_summary,
    format_passthrough_summary, format_shock_report_summary, load_asset_shock_report,
    load_divergence_summary, load_passthrough_summary, ActivityAnomaly, AssetShockReport,
    DailyReturn, DivergenceSummary, HorizonReturn, HorizonReturns, MeasureError,
    MeasureProjectError, PassThroughSummary, ShockScore, ZScoreUnavailable,
    ADEQUATE_BASELINE_OBSERVATIONS, MIN_BASELINE_OBSERVATIONS,
};
pub use project::{
    load_project, validate_project, DataProvenanceMeta, ProjectConfig, ProjectError, ResponseConfig,
};
pub use provenance::{hash_bytes, hash_file, ProvenanceRecord};
pub use report::{
    analyze_project, compare_projects, flows_view, format_compare_table, format_flows_summary,
    format_respond_summary, format_summary, ladder_status, respond_view, AnalysisResult, LadderRow,
    SectionLadder, SectionView, WindowCoverage, FLOW_METRIC_ID, RESPONSE_METRIC_ID,
};
pub use response::{account_market_response, ResponseObservation, ResponseSeriesSummary};
pub use route::{
    DenominatorPolicy, DocStatus, ExecStatus, FlowUnitConfig, LinkageClass, MeasuredLeg,
    ObservedStatus, Restriction, Route, RouteEvidence, RouteMeasurement, RouteMechanism,
};
