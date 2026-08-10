//! Project configuration load and validation (schema v2: per-route measurement).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::control::{ControlAsset, ControlRelation};
use crate::coverage::{CoverageGap, MissingKind};
use crate::event::{Event, EventWindow};
use crate::flow::AttributionMethod;
use crate::identity::{AssetId, AssetKey, AssetLocator, CanonicalAsset, ChainId, ProductKind};
use crate::route::{
    DenominatorPolicy, DocStatus, ExecStatus, FlowUnitConfig, LinkageClass, MeasuredLeg,
    ObservedStatus, Restriction, Route, RouteEvidence, RouteMeasurement, RouteMechanism,
};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error(
        "unsupported schema_version {0}; expected {SCHEMA_VERSION} (v1 global [flow] removed — use routes.measurement)"
    )]
    UnsupportedSchema(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub schema_version: u32,
    pub project_id: String,
    pub name: String,
    pub event: Event,
    pub windows: Vec<EventWindow>,
    pub assets: Vec<CanonicalAsset>,
    pub routes: Vec<Route>,
    pub route_evidence: Vec<RouteEvidence>,
    pub controls: Vec<ControlAsset>,
    pub coverage_declared: Vec<CoverageGap>,
    pub inputs: InputPaths,
    #[serde(skip)]
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputPaths {
    pub flows: String,
    pub supply: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    schema_version: u32,
    project_id: String,
    name: String,
    event: RawEvent,
    windows: Vec<RawWindow>,
    assets: Vec<RawAsset>,
    routes: Vec<RawRoute>,
    #[serde(default)]
    route_evidence: Vec<RawRouteEvidence>,
    /// Removed in schema v2. Presence is a hard error with a migration hint.
    #[serde(default)]
    flow: Option<toml::Value>,
    #[serde(default)]
    controls: Vec<RawControl>,
    #[serde(default)]
    coverage_declared: Vec<RawCoverage>,
    inputs: RawInputs,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    name: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RawWindow {
    name: String,
    start: NaiveDate,
    end: NaiveDate,
}

#[derive(Debug, Deserialize)]
struct RawAsset {
    key: String,
    chain: String,
    #[serde(flatten)]
    locator: RawLocator,
    display_symbol: String,
    issuer: Option<String>,
    #[serde(default = "default_product_kind")]
    product_kind: String,
    underlying_ref: Option<String>,
    role: Option<String>,
}

fn default_product_kind() -> String {
    "unknown".into()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawLocator {
    Mint {
        mint: String,
    },
    Erc20 {
        erc20: String,
    },
    Cex {
        cex_venue: String,
        cex_symbol: String,
    },
    Opaque {
        opaque_id: String,
    },
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    id: String,
    source: String,
    destination: String,
    mechanism: String,
    measurement: RawMeasurement,
}

#[derive(Debug, Deserialize)]
struct RawMeasurement {
    unit: String,
    unit_asset: String,
    measured_leg: String,
    attribution: String,
    denominator: Option<RawDenominator>,
}

#[derive(Debug, Deserialize)]
struct RawRouteEvidence {
    route_id: String,
    documented: String,
    technically_executable: String,
    #[serde(default = "default_unknown")]
    observed_on_chain: String,
    #[serde(default)]
    restrictions: Vec<String>,
    #[serde(default = "default_unknown")]
    linkage_class: String,
    notes: Option<String>,
}

fn default_unknown() -> String {
    "unknown".into()
}

#[derive(Debug, Deserialize)]
struct RawDenominator {
    #[serde(rename = "type")]
    kind: String,
    asset: String,
    as_of: NaiveDate,
}

#[derive(Debug, Deserialize)]
struct RawControl {
    key: String,
    relation: String,
}

#[derive(Debug, Deserialize)]
struct RawCoverage {
    kind: String,
    scope: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct RawInputs {
    flows: String,
    supply: Option<String>,
}

/// Load and validate a project directory containing `project.toml`.
pub fn load_project(project_dir: impl AsRef<Path>) -> Result<ProjectConfig, ProjectError> {
    let root = project_dir.as_ref().canonicalize()?;
    let config_path = root.join("project.toml");
    let text = fs::read_to_string(&config_path)?;
    let raw: RawProject = toml::from_str(&text)?;
    if raw.schema_version != SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedSchema(raw.schema_version));
    }
    if raw.flow.is_some() {
        return Err(ProjectError::Validation(
            "global [flow] was removed in schema v2; declare measurement on each [[routes]] entry"
                .into(),
        ));
    }
    let cfg = map_raw(raw, root)?;
    validate_project(&cfg)?;
    Ok(cfg)
}

fn map_raw(raw: RawProject, root: PathBuf) -> Result<ProjectConfig, ProjectError> {
    let assets = raw
        .assets
        .into_iter()
        .map(|a| {
            let locator = match a.locator {
                RawLocator::Mint { mint } => AssetLocator::Mint { address: mint },
                RawLocator::Erc20 { erc20 } => AssetLocator::Erc20 { address: erc20 },
                RawLocator::Cex {
                    cex_venue,
                    cex_symbol,
                } => AssetLocator::CexSymbol {
                    venue: cex_venue,
                    symbol: cex_symbol,
                },
                RawLocator::Opaque { opaque_id } => AssetLocator::Opaque { id: opaque_id },
            };
            Ok(CanonicalAsset {
                key: AssetKey::new(a.key),
                id: AssetId {
                    chain: ChainId::new(a.chain),
                    locator,
                },
                display_symbol: a.display_symbol,
                issuer: a.issuer,
                product_kind: parse_product_kind(&a.product_kind),
                underlying_ref: a.underlying_ref,
                role: a.role,
            })
        })
        .collect::<Result<Vec<_>, ProjectError>>()?;

    let routes = raw
        .routes
        .into_iter()
        .map(|r| {
            Ok(Route {
                id: r.id,
                source: AssetKey::new(r.source),
                destination: AssetKey::new(r.destination),
                mechanism: parse_mechanism(&r.mechanism)?,
                measurement: map_measurement(r.measurement)?,
            })
        })
        .collect::<Result<Vec<_>, ProjectError>>()?;

    let route_evidence = raw
        .route_evidence
        .into_iter()
        .map(|e| {
            Ok(RouteEvidence {
                route_id: e.route_id,
                documented: parse_doc(&e.documented)?,
                technically_executable: parse_exec(&e.technically_executable)?,
                observed_on_chain: parse_observed(&e.observed_on_chain)?,
                restrictions: e
                    .restrictions
                    .into_iter()
                    .map(|s| parse_restriction(&s))
                    .collect(),
                linkage_class: parse_linkage(&e.linkage_class)?,
                notes: e.notes,
            })
        })
        .collect::<Result<Vec<_>, ProjectError>>()?;

    let controls = raw
        .controls
        .into_iter()
        .map(|c| ControlAsset {
            key: AssetKey::new(c.key),
            relation: parse_control_relation(&c.relation),
        })
        .collect();

    let coverage_declared = raw
        .coverage_declared
        .into_iter()
        .map(|c| CoverageGap {
            kind: parse_missing_kind(&c.kind),
            scope: c.scope,
            reason: c.reason,
        })
        .collect();

    Ok(ProjectConfig {
        schema_version: raw.schema_version,
        project_id: raw.project_id,
        name: raw.name,
        event: Event {
            id: raw.event.id,
            name: raw.event.name,
            timestamp: raw.event.timestamp,
        },
        windows: raw
            .windows
            .into_iter()
            .map(|w| EventWindow {
                name: w.name,
                start: w.start,
                end: w.end,
            })
            .collect(),
        assets,
        routes,
        route_evidence,
        controls,
        coverage_declared,
        inputs: InputPaths {
            flows: raw.inputs.flows,
            supply: raw.inputs.supply,
        },
        root,
    })
}

fn map_measurement(raw: RawMeasurement) -> Result<RouteMeasurement, ProjectError> {
    let unit = match raw.unit.as_str() {
        "token_native" => FlowUnitConfig::TokenNative,
        "quote_usd" => FlowUnitConfig::QuoteUsd,
        "unknown" => FlowUnitConfig::Unknown,
        other => {
            return Err(ProjectError::Validation(format!(
                "unknown measurement.unit '{other}'"
            )))
        }
    };
    let measured_leg = match raw.measured_leg.as_str() {
        "source" => MeasuredLeg::Source,
        "destination" => MeasuredLeg::Destination,
        other => {
            return Err(ProjectError::Validation(format!(
                "unrecognized measurement.measured_leg '{other}' (expected source|destination)"
            )))
        }
    };
    let attribution = parse_attribution(&raw.attribution)?;
    let denominator = match raw.denominator {
        None => None,
        Some(d) => {
            if d.kind != "supply_snapshot" {
                return Err(ProjectError::Validation(format!(
                    "unknown denominator type '{}'",
                    d.kind
                )));
            }
            Some(DenominatorPolicy::SupplySnapshot {
                asset: AssetKey::new(d.asset),
                as_of: d.as_of,
            })
        }
    };
    Ok(RouteMeasurement {
        unit,
        unit_asset: AssetKey::new(raw.unit_asset),
        measured_leg,
        attribution,
        denominator,
    })
}

pub fn validate_project(cfg: &ProjectConfig) -> Result<(), ProjectError> {
    if cfg.project_id.trim().is_empty() {
        return Err(ProjectError::Validation(
            "project_id must be non-empty".into(),
        ));
    }

    if cfg.windows.is_empty() {
        return Err(ProjectError::Validation(
            "at least one analysis window is required".into(),
        ));
    }

    let mut window_names = HashSet::new();
    for window in &cfg.windows {
        window.validate().map_err(ProjectError::Validation)?;
        if !window_names.insert(window.name.clone()) {
            return Err(ProjectError::Validation(format!(
                "duplicate window name '{}'",
                window.name
            )));
        }
    }

    let mut keys = HashSet::new();
    let mut ids = HashSet::new();
    for asset in &cfg.assets {
        if !keys.insert(asset.key.clone()) {
            return Err(ProjectError::Validation(format!(
                "duplicate asset key '{}'",
                asset.key
            )));
        }
        if !ids.insert(asset.id.clone()) {
            return Err(ProjectError::Validation(format!(
                "duplicate asset id for key '{}' (ticker is not identity; locator collision)",
                asset.key
            )));
        }
    }

    let mut route_ids = HashSet::new();
    let mut any_denom = false;

    for route in &cfg.routes {
        if !route_ids.insert(route.id.clone()) {
            return Err(ProjectError::Validation(format!(
                "duplicate route id '{}'",
                route.id
            )));
        }
        if !keys.contains(&route.source) {
            return Err(ProjectError::Validation(format!(
                "route '{}': unknown source asset '{}'",
                route.id, route.source
            )));
        }
        if !keys.contains(&route.destination) {
            return Err(ProjectError::Validation(format!(
                "route '{}': unknown destination asset '{}'",
                route.id, route.destination
            )));
        }
        if route.source == route.destination {
            return Err(ProjectError::Validation(format!(
                "route '{}': source and destination are identical",
                route.id
            )));
        }

        validate_route_measurement(route, &keys)?;
        if route.measurement.denominator.is_some() {
            any_denom = true;
        }
    }

    if any_denom && cfg.inputs.supply.is_none() {
        return Err(ProjectError::Validation(
            "at least one route denominator requires inputs.supply".into(),
        ));
    }

    for evidence in &cfg.route_evidence {
        if !route_ids.contains(&evidence.route_id) {
            return Err(ProjectError::Validation(format!(
                "route_evidence refers to unknown route '{}'",
                evidence.route_id
            )));
        }
    }

    for control in &cfg.controls {
        if !keys.contains(&control.key) {
            return Err(ProjectError::Validation(format!(
                "control key '{}' is not a declared asset",
                control.key
            )));
        }
    }

    let flows_path = cfg.root.join(&cfg.inputs.flows);
    if !flows_path.is_file() {
        return Err(ProjectError::Validation(format!(
            "flows input not found: {}",
            flows_path.display()
        )));
    }
    if let Some(supply) = &cfg.inputs.supply {
        let supply_path = cfg.root.join(supply);
        if !supply_path.is_file() {
            return Err(ProjectError::Validation(format!(
                "supply input not found: {}",
                supply_path.display()
            )));
        }
    }

    Ok(())
}

/// Bind route.source/destination ↔ measured_leg ↔ unit_asset ↔ denominator.
fn validate_route_measurement(route: &Route, keys: &HashSet<AssetKey>) -> Result<(), ProjectError> {
    let m = &route.measurement;

    if !keys.contains(&m.unit_asset) {
        return Err(ProjectError::Validation(format!(
            "route '{}': measurement.unit_asset '{}' is not a declared asset",
            route.id, m.unit_asset
        )));
    }

    let expected = route.measured_asset();
    if &m.unit_asset != expected {
        return Err(ProjectError::Validation(format!(
            "route '{}': measurement.unit_asset '{}' must equal measured_leg asset '{}' ({:?})",
            route.id, m.unit_asset, expected, m.measured_leg
        )));
    }

    if let Some(DenominatorPolicy::SupplySnapshot { asset, .. }) = &m.denominator {
        if !keys.contains(asset) {
            return Err(ProjectError::Validation(format!(
                "route '{}': denominator asset '{}' is not declared",
                route.id, asset
            )));
        }
        match m.unit {
            FlowUnitConfig::TokenNative => {
                if asset != &m.unit_asset {
                    return Err(ProjectError::Validation(format!(
                        "route '{}': denominator asset '{}' must equal measurement.unit_asset '{}'",
                        route.id, asset, m.unit_asset
                    )));
                }
            }
            FlowUnitConfig::QuoteUsd => {
                return Err(ProjectError::Validation(format!(
                    "route '{}': supply_snapshot denominator is incompatible with quote_usd unit",
                    route.id
                )));
            }
            FlowUnitConfig::Unknown => {
                return Err(ProjectError::Validation(format!(
                    "route '{}': supply_snapshot denominator requires a known unit (token_native)",
                    route.id
                )));
            }
        }
    }

    Ok(())
}

fn parse_product_kind(s: &str) -> ProductKind {
    match s {
        "wrapper" => ProductKind::Wrapper,
        "spot_share" => ProductKind::SpotShare,
        "loan_participation" => ProductKind::LoanParticipation,
        "unknown" => ProductKind::Unknown,
        other => ProductKind::Other(other.into()),
    }
}

fn parse_mechanism(s: &str) -> Result<RouteMechanism, ProjectError> {
    match s {
        "swap_pair" => Ok(RouteMechanism::SwapPair),
        "issuer_conversion" => Ok(RouteMechanism::IssuerConversion),
        "bridge" => Ok(RouteMechanism::Bridge),
        "burn_mint" => Ok(RouteMechanism::BurnMint),
        "unknown" => Ok(RouteMechanism::Unknown),
        other => Ok(RouteMechanism::Other(other.into())),
    }
}

fn parse_attribution(s: &str) -> Result<AttributionMethod, ProjectError> {
    match s {
        "fixture" => Ok(AttributionMethod::Fixture),
        "mint_pair_swaps" => Ok(AttributionMethod::MintPairSwaps),
        other => Err(ProjectError::Validation(format!(
            "unrecognized measurement.attribution '{other}' (expected fixture|mint_pair_swaps)"
        ))),
    }
}

fn parse_doc(s: &str) -> Result<DocStatus, ProjectError> {
    match s {
        "issuer_named" => Ok(DocStatus::IssuerNamed),
        "docs_mentioned" => Ok(DocStatus::DocsMentioned),
        "none" => Ok(DocStatus::None),
        "unknown" => Ok(DocStatus::Unknown),
        other => Err(ProjectError::Validation(format!(
            "unrecognized route_evidence.documented '{other}'"
        ))),
    }
}

fn parse_exec(s: &str) -> Result<ExecStatus, ProjectError> {
    match s {
        "permissionless" => Ok(ExecStatus::Permissionless),
        "gated" => Ok(ExecStatus::Gated),
        "unknown" => Ok(ExecStatus::Unknown),
        other => Err(ProjectError::Validation(format!(
            "unrecognized route_evidence.technically_executable '{other}'"
        ))),
    }
}

fn parse_observed(s: &str) -> Result<ObservedStatus, ProjectError> {
    match s {
        "yes" => Ok(ObservedStatus::Yes),
        "no_in_window" => Ok(ObservedStatus::NoInWindow),
        "unknown" => Ok(ObservedStatus::Unknown),
        other => Err(ProjectError::Validation(format!(
            "unrecognized route_evidence.observed_on_chain '{other}' (use no_in_window, not 'no')"
        ))),
    }
}

fn parse_linkage(s: &str) -> Result<LinkageClass, ProjectError> {
    match s {
        "direct_same_tx" => Ok(LinkageClass::DirectSameTx),
        "mint_pair_match" => Ok(LinkageClass::MintPairMatch),
        "issuer_two_leg" => Ok(LinkageClass::IssuerTwoLeg),
        "unlinked" => Ok(LinkageClass::Unlinked),
        "unknown" => Ok(LinkageClass::Unknown),
        other => Err(ProjectError::Validation(format!(
            "unrecognized route_evidence.linkage_class '{other}'"
        ))),
    }
}

fn parse_restriction(s: &str) -> Restriction {
    match s {
        "kyc" => Restriction::Kyc,
        "min_size" => Restriction::MinSize,
        "jurisdiction" => Restriction::Jurisdiction,
        "deadline" => Restriction::Deadline,
        "whitelist" => Restriction::Whitelist,
        other => Restriction::Other(other.into()),
    }
}

fn parse_control_relation(s: &str) -> ControlRelation {
    match s {
        "same_issuer_different_underlying" => ControlRelation::SameIssuerDifferentUnderlying,
        "same_underlying_different_issuer" => ControlRelation::SameUnderlyingDifferentIssuer,
        "same_chain_venue" => ControlRelation::SameChainVenue,
        "similar_wrapper_type" => ControlRelation::SimilarWrapperType,
        "unaffected" => ControlRelation::Unaffected,
        other => ControlRelation::Other(other.into()),
    }
}

fn parse_missing_kind(s: &str) -> MissingKind {
    match s {
        "off_chain_venue" => MissingKind::OffChainVenue,
        "historical_depth" => MissingKind::HistoricalDepth,
        "route_attribution" => MissingKind::RouteAttribution,
        "supply" => MissingKind::Supply,
        "trader_intent" => MissingKind::TraderIntent,
        "actor_linkage" => MissingKind::ActorLinkage,
        "price" => MissingKind::Price,
        other => MissingKind::Other(other.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn synthetic_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/synthetic_conduit")
            .canonicalize()
            .expect("tests/synthetic_conduit must exist")
    }

    #[test]
    fn loads_and_validates_synthetic_project() {
        let cfg = load_project(synthetic_root()).unwrap();
        assert_eq!(cfg.project_id, "synthetic_conduit");
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.assets.len(), 3);
        assert_eq!(cfg.routes.len(), 1);
        assert_eq!(
            cfg.routes[0].measurement.attribution,
            AttributionMethod::Fixture
        );
        assert_eq!(cfg.routes[0].measurement.measured_leg, MeasuredLeg::Source);
    }

    #[test]
    fn shared_display_symbol_different_locator_ok() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.assets[1].display_symbol = cfg.assets[0].display_symbol.clone();
        validate_project(&cfg).unwrap();
    }

    #[test]
    fn rejects_denominator_asset_mismatch() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.routes[0].measurement.denominator = Some(DenominatorPolicy::SupplySnapshot {
            asset: AssetKey::new("B"),
            as_of: NaiveDate::from_ymd_opt(2026, 6, 11).unwrap(),
        });
        let err = validate_project(&cfg).unwrap_err();
        assert!(err
            .to_string()
            .contains("must equal measurement.unit_asset"));
    }

    #[test]
    fn rejects_unit_asset_not_equal_measured_leg() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        // measured_leg = source (A), but unit_asset forced to B
        cfg.routes[0].measurement.unit_asset = AssetKey::new("B");
        cfg.routes[0].measurement.denominator = Some(DenominatorPolicy::SupplySnapshot {
            asset: AssetKey::new("B"),
            as_of: NaiveDate::from_ymd_opt(2026, 6, 11).unwrap(),
        });
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("must equal measured_leg asset"));
    }

    #[test]
    fn rejects_unrecognized_observed_status() {
        let err = parse_observed("no").unwrap_err();
        assert!(err.to_string().contains("observed_on_chain"));
    }

    #[test]
    fn rejects_dangling_route_evidence() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.route_evidence[0].route_id = "missing_route".into();
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("unknown route"));
    }

    #[test]
    fn rejects_self_referential_route() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.routes[0].destination = cfg.routes[0].source.clone();
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("identical"));
    }

    #[test]
    fn rejects_window_end_before_start() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.windows[0].end = cfg.windows[0].start.pred_opt().unwrap();
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("before start"));
    }

    #[test]
    fn rejects_undeclared_control_key() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.controls[0].key = AssetKey::new("Z_MISSING");
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("control key"));
    }

    #[test]
    fn rejects_undeclared_unit_asset() {
        let mut cfg = load_project(synthetic_root()).unwrap();
        cfg.routes[0].measurement.unit_asset = AssetKey::new("Z_MISSING");
        let err = validate_project(&cfg).unwrap_err();
        assert!(err.to_string().contains("unit_asset"));
    }
}
