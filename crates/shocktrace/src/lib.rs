//! Shocktrace: deterministic measurement of market responses and directional flows.
//!
//! This crate measures empirical objects. It does **not** infer migration from
//! coincident activity changes (`A↓ + B↑ ⇏ capital moved A→B`).

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod control;
pub mod coverage;
pub mod event;
pub mod flow;
pub mod identity;
pub mod ingest;
pub mod observe;
pub mod project;
pub mod provenance;
pub mod report;
pub mod route;

pub use control::{ControlAsset, ControlRelation};
pub use coverage::{CoverageGap, EvidenceBoundary, MissingKind};
pub use event::{Event, EventWindow};
pub use flow::{
    account_directional_flows, AttributionMethod, DirectionalFlowObservation, FlowSeriesSummary,
    FlowUnit, Quantity,
};
pub use identity::{
    AssetId, AssetKey, AssetLocator, CanonicalAsset, ChainId, ProductKind, VenueId,
};
pub use project::{load_project, validate_project, ProjectConfig, ProjectError};
pub use provenance::{hash_bytes, hash_file, ProvenanceRecord};
pub use report::{analyze_project, AnalysisResult};
pub use route::{
    DenominatorPolicy, DocStatus, ExecStatus, FlowUnitConfig, LinkageClass, MeasuredLeg,
    ObservedStatus, Restriction, Route, RouteEvidence, RouteMeasurement, RouteMechanism,
};
