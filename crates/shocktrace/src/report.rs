//! Assemble machine-readable analysis results.
//!
//! Market response, route evidence, and directional flow are separate sections.
//! Absence is never encoded as zero.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::Utc;
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

use crate::coverage::{CoverageGap, EvidenceBoundary, GapSource, MissingKind};
use crate::evidence::EvidenceSection;
use crate::flow::{
    account_directional_flows, inclusive_day_count, DirectionalFlowObservation, FlowSeriesSummary,
    FlowUnit,
};
use crate::ingest::daily_flows::load_daily_flow_rows;
use crate::ingest::daily_response::load_daily_response;
use crate::ingest::daily_supply::{load_daily_supply, supply_on};
use crate::project::{ProjectConfig, ProjectError};
use crate::provenance::{hash_bytes, hash_file, InputHash, ProvenanceRecord};
use crate::response::{account_market_response, ResponseObservation, ResponseSeriesSummary};
use crate::route::{DenominatorPolicy, FlowUnitConfig, Route, RouteEvidence};

const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FLOW_METRIC_ID: &str = "directional_flow.v2";
pub const RESPONSE_METRIC_ID: &str = "market_response.v1";

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Flow(#[from] crate::flow::FlowError),
    #[error(transparent)]
    Response(#[from] crate::response::ResponseError),
    #[error("ingest error: {0}")]
    Ingest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub project_id: String,
    pub engine_version: String,
    pub market_response: EvidenceSection<Vec<ResponseSeriesSummary>>,
    pub route_evidence: EvidenceSection<Vec<RouteEvidence>>,
    pub directional_flow: EvidenceSection<Vec<FlowSeriesSummary>>,
    pub boundary: EvidenceBoundary,
    pub provenance: ProvenanceRecord,
}

pub fn analyze_project(cfg: &ProjectConfig, command: &str) -> Result<AnalysisResult, AnalyzeError> {
    let mut boundary = EvidenceBoundary::default();
    boundary.merge_declared(cfg.coverage_declared.clone());
    boundary.assume(
        "Engine outputs measurement sections only. Lack of flow evidence is not a migration verdict.",
    );

    let market_response = compute_market_response(cfg, &mut boundary)?;
    let route_evidence = compute_route_evidence(cfg);
    let directional_flow = compute_directional_flow(cfg, &mut boundary)?;
    let provenance = build_provenance(cfg, command)?;

    Ok(AnalysisResult {
        project_id: cfg.project_id.clone(),
        engine_version: ENGINE_VERSION.to_string(),
        market_response,
        route_evidence,
        directional_flow,
        boundary,
        provenance,
    })
}

fn compute_route_evidence(cfg: &ProjectConfig) -> EvidenceSection<Vec<RouteEvidence>> {
    if cfg.routes.is_empty() {
        EvidenceSection::not_declared(
            "no [[routes]] declared; route evidence ladder step not applicable",
        )
    } else if cfg.route_evidence.is_empty() {
        EvidenceSection::not_observable("routes declared but no [[route_evidence]] rows provided")
    } else {
        EvidenceSection::available(cfg.route_evidence.clone())
    }
}

fn compute_market_response(
    cfg: &ProjectConfig,
    boundary: &mut EvidenceBoundary,
) -> Result<EvidenceSection<Vec<ResponseSeriesSummary>>, AnalyzeError> {
    let Some(rel) = &cfg.inputs.response else {
        return Ok(EvidenceSection::not_declared(
            "inputs.response not set; market-response section not requested",
        ));
    };
    let Some(resp_cfg) = &cfg.response else {
        return Ok(EvidenceSection::not_declared(
            "[response] config missing despite inputs.response",
        ));
    };

    let path = cfg.root.join(rel);
    let rows = load_daily_response(&path).map_err(|e| AnalyzeError::Ingest(e.to_string()))?;

    let asset_keys: BTreeSet<_> = cfg.assets.iter().map(|a| a.key.clone()).collect();
    let mut by_asset: BTreeMap<_, Vec<ResponseObservation>> = BTreeMap::new();
    let mut undeclared_assets = BTreeSet::new();
    for row in rows {
        if !asset_keys.contains(&row.asset_key) {
            undeclared_assets.insert(row.asset_key);
            continue;
        }
        by_asset.entry(row.asset_key.clone()).or_default().push(row);
    }
    for asset in undeclared_assets {
        boundary.push_detected(CoverageGap::new(
            MissingKind::Other("undeclared_response_asset".into()),
            asset.as_str().to_string(),
            "response row asset_key not in project assets",
        ));
    }

    let baseline_window = cfg
        .windows
        .iter()
        .find(|w| w.name == resp_cfg.baseline_window)
        .expect("validated baseline_window");

    let mut summaries = Vec::new();
    for (asset_key, series) in &by_asset {
        let baseline: Vec<_> = series
            .iter()
            .filter(|o| baseline_window.contains(o.day))
            .cloned()
            .collect();

        for window in &cfg.windows {
            let in_window: Vec<_> = series
                .iter()
                .filter(|o| window.contains(o.day))
                .cloned()
                .collect();
            if in_window.is_empty() {
                boundary.push_detected(CoverageGap::new(
                    MissingKind::Other("response_coverage".into()),
                    format!("{}@{}", asset_key, window.name),
                    format!(
                        "no response observations in window '{}' (0 of {} days)",
                        window.name,
                        inclusive_day_count(window)
                    ),
                ));
                continue;
            }
            let mut summary = account_market_response(in_window, window, &baseline)?;
            // Self-normalization against the same window is definitionally 1 — omit.
            if window.name == resp_cfg.baseline_window {
                summary.baseline_normalized_volume = None;
            }
            if summary.observed_days < summary.window_days {
                boundary.push_detected(CoverageGap::new(
                    MissingKind::Other("response_coverage".into()),
                    format!("{}@{}", asset_key, window.name),
                    format!(
                        "{} of {} days missing in window '{}'",
                        summary.window_days - summary.observed_days,
                        summary.window_days,
                        window.name
                    ),
                ));
            }
            summaries.push(summary);
        }
    }

    if summaries.is_empty() {
        return Ok(EvidenceSection::not_observable(
            "response input present but no in-window observations for declared assets",
        ));
    }

    Ok(EvidenceSection::available(summaries))
}

fn compute_directional_flow(
    cfg: &ProjectConfig,
    boundary: &mut EvidenceBoundary,
) -> Result<EvidenceSection<Vec<FlowSeriesSummary>>, AnalyzeError> {
    if cfg.routes.is_empty() {
        return Ok(EvidenceSection::not_declared(
            "no [[routes]] declared; directional flow not identified (not zero)",
        ));
    }

    let Some(flows_rel) = &cfg.inputs.flows else {
        return Ok(EvidenceSection::not_observable(
            "routes declared but inputs.flows missing",
        ));
    };

    let flows_path = cfg.root.join(flows_rel);
    let raw_rows =
        load_daily_flow_rows(&flows_path).map_err(|e| AnalyzeError::Ingest(e.to_string()))?;

    let routes_by_id: BTreeMap<&str, &Route> =
        cfg.routes.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut by_route: BTreeMap<String, Vec<DirectionalFlowObservation>> = BTreeMap::new();
    let mut undeclared_routes = BTreeSet::new();

    for row in raw_rows {
        let Some(route) = routes_by_id.get(row.route_id.as_str()) else {
            undeclared_routes.insert(row.route_id);
            continue;
        };
        let unit = flow_unit_from_measurement(route);
        by_route
            .entry(row.route_id.clone())
            .or_default()
            .push(DirectionalFlowObservation {
                route_id: row.route_id,
                day: row.day,
                gross_a_to_b: row.gross_a_to_b,
                gross_b_to_a: row.gross_b_to_a,
                unit,
                attribution: route.measurement.attribution.clone(),
            });
    }

    let supply_rows = if cfg
        .routes
        .iter()
        .any(|r| r.measurement.denominator.is_some())
    {
        match &cfg.inputs.supply {
            Some(rel) => Some(
                load_daily_supply(&cfg.root.join(rel))
                    .map_err(|e| AnalyzeError::Ingest(e.to_string()))?,
            ),
            None => None,
        }
    } else {
        None
    };

    let mut flow_summaries = Vec::new();

    for route in &cfg.routes {
        let denominator = resolve_route_denominator(route, supply_rows.as_deref(), boundary)?;

        let Some(series) = by_route.remove(&route.id) else {
            for window in &cfg.windows {
                boundary.push_detected(CoverageGap::new(
                    MissingKind::RouteAttribution,
                    format!("{}@{}", route.id, window.name),
                    format!(
                        "no flow observations found for route in window '{}' (0 of {} days)",
                        window.name,
                        inclusive_day_count(window)
                    ),
                ));
            }
            continue;
        };

        for window in &cfg.windows {
            let in_window: Vec<_> = series
                .iter()
                .filter(|o| window.contains(o.day))
                .cloned()
                .collect();

            let window_days = inclusive_day_count(window);
            if in_window.is_empty() {
                boundary.push_detected(CoverageGap::new(
                    MissingKind::RouteAttribution,
                    format!("{}@{}", route.id, window.name),
                    format!(
                        "no flow observations in window '{}' (0 of {window_days} days)",
                        window.name
                    ),
                ));
                continue;
            }

            let summary = account_directional_flows(in_window, window, denominator)?;
            if summary.observed_days < summary.window_days {
                boundary.push_detected(CoverageGap::new(
                    MissingKind::RouteAttribution,
                    format!("{}@{}", route.id, window.name),
                    format!(
                        "{} of {} days missing in window '{}'",
                        summary.window_days - summary.observed_days,
                        summary.window_days,
                        window.name
                    ),
                ));
            }
            flow_summaries.push(summary);
        }

        let outside_days: BTreeSet<_> = series
            .iter()
            .filter(|o| !cfg.windows.iter().any(|w| w.contains(o.day)))
            .map(|o| o.day)
            .collect();
        if !outside_days.is_empty() {
            let sample: Vec<_> = outside_days.iter().take(5).map(|d| d.to_string()).collect();
            boundary.push_detected(CoverageGap::new(
                MissingKind::RouteAttribution,
                route.id.clone(),
                format!(
                    "{} observation day(s) outside all declared windows (e.g. {})",
                    outside_days.len(),
                    sample.join(", ")
                ),
            ));
        }
    }

    for extra_route in undeclared_routes {
        boundary.push_detected(CoverageGap::new(
            MissingKind::RouteAttribution,
            extra_route,
            "flow observations reference a route_id not declared in project.toml",
        ));
    }

    if flow_summaries.is_empty() {
        return Ok(EvidenceSection::not_observable(
            "routes declared but no in-window flow observations were accounted",
        ));
    }

    Ok(EvidenceSection::available(flow_summaries))
}

fn flow_unit_from_measurement(route: &Route) -> FlowUnit {
    match route.measurement.unit {
        FlowUnitConfig::TokenNative => FlowUnit::TokenNative {
            asset: route.measurement.unit_asset.clone(),
        },
        FlowUnitConfig::QuoteUsd => FlowUnit::QuoteUsd,
        FlowUnitConfig::Unknown => FlowUnit::Unknown,
    }
}

fn resolve_route_denominator(
    route: &Route,
    supply_rows: Option<&[(crate::identity::AssetKey, chrono::NaiveDate, Decimal)]>,
    boundary: &mut EvidenceBoundary,
) -> Result<Option<Decimal>, AnalyzeError> {
    let Some(DenominatorPolicy::SupplySnapshot { asset, as_of }) = &route.measurement.denominator
    else {
        return Ok(None);
    };

    let Some(rows) = supply_rows else {
        boundary.push_detected(CoverageGap::new(
            MissingKind::Supply,
            format!("{}:{}", route.id, asset),
            "denominator requested but inputs.supply missing",
        ));
        return Ok(None);
    };

    match supply_on(rows, asset, *as_of) {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            boundary.push_detected(CoverageGap::new(
                MissingKind::Supply,
                format!("{}:{}", route.id, asset),
                e.to_string(),
            ));
            Ok(None)
        }
    }
}

fn build_provenance(cfg: &ProjectConfig, command: &str) -> Result<ProvenanceRecord, AnalyzeError> {
    let config_bytes = fs::read(cfg.root.join("project.toml"))?;
    let mut input_hashes = vec![InputHash {
        path: "project.toml".into(),
        sha256: hash_bytes(&config_bytes),
    }];

    for rel in [
        cfg.inputs.flows.as_deref(),
        cfg.inputs.supply.as_deref(),
        cfg.inputs.response.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        input_hashes.push(InputHash {
            path: rel.to_string(),
            sha256: hash_file(Path::new(&cfg.root.join(rel)))?,
        });
    }

    let metric = if cfg.inputs.response.is_some() && !cfg.routes.is_empty() {
        format!("{RESPONSE_METRIC_ID}+{FLOW_METRIC_ID}")
    } else if cfg.inputs.response.is_some() {
        RESPONSE_METRIC_ID.to_string()
    } else {
        FLOW_METRIC_ID.to_string()
    };

    Ok(ProvenanceRecord {
        engine_version: ENGINE_VERSION.to_string(),
        project_id: cfg.project_id.clone(),
        config_sha256: hash_bytes(&config_bytes),
        input_hashes,
        metric_definition_id: metric,
        command: command.to_string(),
        computed_at_unix: Utc::now().timestamp(),
    })
}

pub fn format_summary(result: &AnalysisResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("project: {}", result.project_id));
    lines.push(format!("engine: {}", result.engine_version));
    lines.push(format!(
        "metric: {}",
        result.provenance.metric_definition_id
    ));
    lines.push(String::new());

    lines.push(format_section(
        "market_response",
        &result.market_response,
        |data| format_response_rows(data),
    ));
    lines.push(format_section(
        "route_evidence",
        &result.route_evidence,
        |data| format_route_rows(data),
    ));
    lines.push(format_section(
        "directional_flow",
        &result.directional_flow,
        |data| format_flow_rows(data),
    ));

    lines.push(format_boundary_block(&result.boundary));
    lines.join("\n")
}

/// `respond` summary: market-response section + response-related gaps.
pub fn format_respond_summary(result: &AnalysisResult) -> String {
    [
        format_section("market_response", &result.market_response, |data| {
            format_response_rows(data)
        }),
        format_boundary_block(&filter_boundary(&result.boundary, GapFilter::Response)),
    ]
    .join("\n")
}

/// `flows` summary: directional-flow section + flow-related gaps.
pub fn format_flows_summary(result: &AnalysisResult) -> String {
    [
        format_section("directional_flow", &result.directional_flow, |data| {
            format_flow_rows(data)
        }),
        format_boundary_block(&filter_boundary(&result.boundary, GapFilter::Flow)),
    ]
    .join("\n")
}

/// Machine-readable single-section view that keeps coverage gaps visible.
#[derive(Debug, Clone, Serialize)]
pub struct SectionView<T> {
    pub section: EvidenceSection<T>,
    pub boundary: EvidenceBoundary,
}

pub fn respond_view(result: &AnalysisResult) -> SectionView<Vec<ResponseSeriesSummary>> {
    SectionView {
        section: result.market_response.clone(),
        boundary: filter_boundary(&result.boundary, GapFilter::Response),
    }
}

pub fn flows_view(result: &AnalysisResult) -> SectionView<Vec<FlowSeriesSummary>> {
    SectionView {
        section: result.directional_flow.clone(),
        boundary: filter_boundary(&result.boundary, GapFilter::Flow),
    }
}

#[derive(Clone, Copy)]
enum GapFilter {
    Response,
    Flow,
}

fn filter_boundary(boundary: &EvidenceBoundary, filter: GapFilter) -> EvidenceBoundary {
    // Author-declared caveats always pass through. Only runtime-detected gaps
    // are filtered by the section that produced them.
    let missing = boundary
        .missing
        .iter()
        .filter(|g| match g.source {
            GapSource::Declared => true,
            GapSource::Detected => match filter {
                GapFilter::Response => is_response_gap(g),
                GapFilter::Flow => is_flow_gap(g),
            },
        })
        .cloned()
        .collect();
    EvidenceBoundary {
        missing,
        assumptions: boundary.assumptions.clone(),
    }
}

fn is_response_gap(gap: &CoverageGap) -> bool {
    match &gap.kind {
        MissingKind::Price => true,
        MissingKind::Other(s) => s == "response_coverage" || s == "undeclared_response_asset",
        _ => false,
    }
}

fn is_flow_gap(gap: &CoverageGap) -> bool {
    matches!(
        gap.kind,
        MissingKind::RouteAttribution | MissingKind::Supply
    )
}

fn format_response_rows(data: &[ResponseSeriesSummary]) -> Vec<String> {
    data.iter()
        .map(|s| {
            format!(
                "  {} @ {}: price_return={} norm_vol={} observed/window={}/{}",
                s.asset_key,
                s.window_name,
                opt_dec(s.price_return),
                opt_dec(s.baseline_normalized_volume),
                s.observed_days,
                s.window_days
            )
        })
        .collect()
}

fn format_route_rows(data: &[crate::route::RouteEvidence]) -> Vec<String> {
    data.iter()
        .map(|e| {
            format!(
                "  {}: documented={:?} executable={:?} observed={:?} linkage={:?}",
                e.route_id,
                e.documented,
                e.technically_executable,
                e.observed_on_chain,
                e.linkage_class
            )
        })
        .collect()
}

fn format_flow_rows(data: &[FlowSeriesSummary]) -> Vec<String> {
    data.iter()
        .map(|s| {
            format!(
                "  {} @ {}: net={} peak={} trough={} net/denom={} observed/window={}/{}",
                s.route_id,
                s.window_name,
                s.net_total,
                s.peak_cumulative_net,
                s.trough_cumulative_net,
                opt_dec(s.net_over_denominator),
                s.observed_days,
                s.window_days
            )
        })
        .collect()
}

fn format_boundary_block(boundary: &EvidenceBoundary) -> String {
    let mut lines = vec!["evidence boundary:".into()];
    if boundary.missing.is_empty() {
        lines.push("  (no coverage gaps recorded)".into());
    } else {
        for gap in &boundary.missing {
            lines.push(format!("  - {}/{}: {}", gap.kind, gap.scope, gap.reason));
        }
    }
    for a in &boundary.assumptions {
        lines.push(format!("  assumption: {a}"));
    }
    lines.join("\n")
}

fn format_section<T, F>(name: &str, section: &EvidenceSection<T>, fmt: F) -> String
where
    F: Fn(&T) -> Vec<String>,
{
    match section {
        EvidenceSection::Available { data } => {
            let mut lines = vec![format!("{name}: available")];
            lines.extend(fmt(data));
            lines.join("\n") + "\n"
        }
        EvidenceSection::NotDeclared { reason } => {
            format!("{name}: not_declared — {reason}\n")
        }
        EvidenceSection::NotObservable { reason } => {
            format!("{name}: not_observable — {reason}\n")
        }
    }
}

fn opt_dec(v: Option<Decimal>) -> String {
    match v {
        Some(d) => d.to_string(),
        None => "null".into(),
    }
}

/// Compact ladder row for article-facing comparisons.
pub fn ladder_status(result: &AnalysisResult) -> LadderRow {
    LadderRow {
        project_id: result.project_id.clone(),
        market_response: section_label(&result.market_response),
        route_evidence: section_label(&result.route_evidence),
        directional_flow: section_label(&result.directional_flow),
        claim_boundary: claim_boundary(result),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LadderRow {
    pub project_id: String,
    pub market_response: String,
    pub route_evidence: String,
    pub directional_flow: String,
    pub claim_boundary: String,
}

fn section_label<T>(section: &EvidenceSection<T>) -> String {
    match section {
        EvidenceSection::Available { .. } => "observed".into(),
        EvidenceSection::NotDeclared { .. } => "not_declared".into(),
        EvidenceSection::NotObservable { .. } => "not_observable".into(),
    }
}

fn claim_boundary(result: &AnalysisResult) -> String {
    match (
        result.market_response.is_available(),
        result.route_evidence.is_available(),
        result.directional_flow.is_available(),
    ) {
        (true, true, true) => {
            "response + route + directional flow measurable; no migration label emitted".into()
        }
        (true, _, false) => "response only; directional flow not identified".into(),
        (false, _, true) => "directional flow measurable; response not requested".into(),
        (true, false, true) => "response + flow measurable; route evidence incomplete".into(),
        _ => "insufficient declared evidence for linked-flow claims".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::{fs, path::PathBuf};

    fn write_temp(project_toml: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shocktrace-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(dir.join("project.toml"), project_toml).unwrap();
        for (rel, body) in files {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, body).unwrap();
        }
        dir
    }

    fn linked_toml(denom_asset: &str, unit_asset: &str, measured_leg: &str) -> String {
        format!(
            r#"
schema_version = 3
project_id = "tmp"
name = "tmp"
[event]
id = "e"
name = "e"
timestamp = "2026-06-12T14:00:00Z"
[[windows]]
name = "post_event"
start = "2026-06-12"
end = "2026-06-18"
[[assets]]
key = "A"
chain = "solana"
mint = "MintA111111111111111111111111111111111111"
display_symbol = "AAA"
[[assets]]
key = "B"
chain = "solana"
mint = "MintB111111111111111111111111111111111111"
display_symbol = "BBB"
[[routes]]
id = "a_b_swap"
source = "A"
destination = "B"
mechanism = "swap_pair"
measurement = {{ unit = "token_native", unit_asset = "{unit_asset}", measured_leg = "{measured_leg}", attribution = "fixture", denominator = {{ type = "supply_snapshot", asset = "{denom_asset}", as_of = "2026-06-11" }} }}
[[route_evidence]]
route_id = "a_b_swap"
documented = "issuer_named"
technically_executable = "permissionless"
observed_on_chain = "yes"
linkage_class = "mint_pair_match"
[inputs]
flows = "data/flows_daily.csv"
supply = "data/supply_daily.csv"
"#
        )
    }

    #[test]
    fn response_only_project_analyzes_without_zero_flow() {
        let toml = r#"
schema_version = 3
project_id = "gold_fixture"
name = "gold"
[event]
id = "shock"
name = "shock"
timestamp = "2026-06-12T14:00:00Z"
[[windows]]
name = "baseline"
start = "2026-06-01"
end = "2026-06-03"
[[windows]]
name = "event"
start = "2026-06-12"
end = "2026-06-14"
[[assets]]
key = "GC"
chain = "tradfi"
venue = "COMEX"
instrument_id = "GC_FRONT_SYNTH"
display_symbol = "GC"
underlying_ref = "gold"
[response]
baseline_window = "baseline"
[inputs]
response = "data/response_daily.csv"
"#;
        let csv = "\
asset_key,day,price,volume
GC,2026-06-01,100,10
GC,2026-06-02,101,10
GC,2026-06-03,102,10
GC,2026-06-12,110,40
GC,2026-06-13,115,50
GC,2026-06-14,121,30
";
        let dir = write_temp(toml, &[("data/response_daily.csv", csv)]);
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        assert!(matches!(
            result.market_response,
            EvidenceSection::Available { .. }
        ));
        assert!(matches!(
            result.route_evidence,
            EvidenceSection::NotDeclared { .. }
        ));
        assert!(matches!(
            result.directional_flow,
            EvidenceSection::NotDeclared { .. }
        ));
        // Critical: not Available with empty/zero flows
        if let EvidenceSection::Available { data } = &result.directional_flow {
            panic!("unexpected available flow: {data:?}");
        }
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("is_migration"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_mismatched_denominator_at_validate() {
        let flows = "route_id,day,gross_a_to_b,gross_b_to_a\na_b_swap,2026-06-12,10.0,1.0\n";
        let supply = "asset_key,day,supply\nA,2026-06-11,1000.0\nB,2026-06-11,5000.0\n";
        let dir = write_temp(
            &linked_toml("B", "A", "source"),
            &[
                ("data/flows_daily.csv", flows),
                ("data/supply_daily.csv", supply),
            ],
        );
        let err = crate::load_project(&dir).unwrap_err();
        assert!(
            err.to_string()
                .contains("must equal measurement.unit_asset"),
            "got: {err}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unit_asset_not_matching_measured_leg() {
        let flows = "route_id,day,gross_a_to_b,gross_b_to_a\na_b_swap,2026-06-12,10.0,1.0\n";
        let supply = "asset_key,day,supply\nA,2026-06-11,1000.0\nB,2026-06-11,5000.0\n";
        let dir = write_temp(
            &linked_toml("B", "B", "source"),
            &[
                ("data/flows_daily.csv", flows),
                ("data/supply_daily.csv", supply),
            ],
        );
        let err = crate::load_project(&dir).unwrap_err();
        assert!(err.to_string().contains("must equal measured_leg asset"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn multi_route_independent_units_and_denominators() {
        let toml = r#"
schema_version = 3
project_id = "multi"
name = "multi"
[event]
id = "e"
name = "e"
timestamp = "2026-06-12T14:00:00Z"
[[windows]]
name = "post_event"
start = "2026-06-12"
end = "2026-06-12"
[[assets]]
key = "A"
chain = "solana"
mint = "MintA111111111111111111111111111111111111"
display_symbol = "AAA"
[[assets]]
key = "B"
chain = "solana"
mint = "MintB111111111111111111111111111111111111"
display_symbol = "BBB"
[[assets]]
key = "C"
chain = "solana"
mint = "MintC111111111111111111111111111111111111"
display_symbol = "CCC"
[[routes]]
id = "a_b_swap"
source = "A"
destination = "B"
mechanism = "swap_pair"
measurement = { unit = "token_native", unit_asset = "A", measured_leg = "source", attribution = "fixture", denominator = { type = "supply_snapshot", asset = "A", as_of = "2026-06-11" } }
[[routes]]
id = "c_b_swap"
source = "C"
destination = "B"
mechanism = "swap_pair"
measurement = { unit = "token_native", unit_asset = "C", measured_leg = "source", attribution = "fixture", denominator = { type = "supply_snapshot", asset = "C", as_of = "2026-06-11" } }
[inputs]
flows = "data/flows_daily.csv"
supply = "data/supply_daily.csv"
"#;
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2026-06-12,50.0,20.0
c_b_swap,2026-06-12,100.0,60.0
";
        let supply = "asset_key,day,supply\nA,2026-06-11,1000.0\nC,2026-06-11,500.0\n";
        let dir = write_temp(
            toml,
            &[
                ("data/flows_daily.csv", flows),
                ("data/supply_daily.csv", supply),
            ],
        );
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let EvidenceSection::Available { data } = &result.directional_flow else {
            panic!("expected available flow");
        };
        let ab = data.iter().find(|s| s.route_id == "a_b_swap").unwrap();
        let cb = data.iter().find(|s| s.route_id == "c_b_swap").unwrap();
        assert_eq!(ab.net_total, Decimal::from(30));
        assert_eq!(
            ab.net_over_denominator.unwrap(),
            Decimal::from_str("0.03").unwrap()
        );
        assert_eq!(cb.net_total, Decimal::from(40));
        assert_eq!(
            cb.net_over_denominator.unwrap(),
            Decimal::from_str("0.08").unwrap()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_window_coverage_emits_gap() {
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2026-06-12,100.0,20.0
a_b_swap,2026-06-13,10.0,0.0
";
        let supply = "asset_key,day,supply\nA,2026-06-11,1000.0\n";
        let dir = write_temp(
            &linked_toml("A", "A", "source"),
            &[
                ("data/flows_daily.csv", flows),
                ("data/supply_daily.csv", supply),
            ],
        );
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        assert!(result.boundary.missing.iter().any(|g| {
            g.scope == "a_b_swap@post_event" && g.reason.contains("5 of 7 days missing")
        }));
        let flows_txt = format_flows_summary(&result);
        assert!(
            flows_txt.contains("evidence boundary:"),
            "flows summary must expose boundary"
        );
        assert!(
            flows_txt.contains("5 of 7 days missing"),
            "partial coverage must appear in flows summary, got:\n{flows_txt}"
        );
        assert!(flows_txt.contains("observed/window=2/7"));
        assert!(!flows_txt.contains("Some("));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn out_of_window_observations_emit_gap_and_exclude_from_totals() {
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2019-01-01,1000.0,0.0
a_b_swap,2031-12-31,800.0,20.0
a_b_swap,2026-06-12,10.0,1.0
";
        let supply = "asset_key,day,supply\nA,2026-06-11,1000.0\n";
        let dir = write_temp(
            &linked_toml("A", "A", "source"),
            &[
                ("data/flows_daily.csv", flows),
                ("data/supply_daily.csv", supply),
            ],
        );
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let EvidenceSection::Available { data } = &result.directional_flow else {
            panic!("expected available flow");
        };
        let post = data.iter().find(|s| s.window_name == "post_event").unwrap();
        assert_eq!(post.observed_days, 1);
        assert_eq!(post.net_total, Decimal::from(9));
        assert!(result.boundary.missing.iter().any(|g| {
            g.scope == "a_b_swap" && g.reason.contains("outside all declared windows")
        }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn undeclared_response_assets_deduped_in_boundary() {
        let toml = r#"
schema_version = 3
project_id = "dedupe"
name = "dedupe"
[event]
id = "e"
name = "e"
timestamp = "2026-06-12T14:00:00Z"
[[windows]]
name = "baseline"
start = "2026-06-01"
end = "2026-06-02"
[[windows]]
name = "event"
start = "2026-06-12"
end = "2026-06-12"
[[assets]]
key = "GC"
chain = "tradfi"
venue = "COMEX"
instrument_id = "GC_FRONT_SYNTH"
display_symbol = "GC"
[response]
baseline_window = "baseline"
[inputs]
response = "data/response_daily.csv"
"#;
        let csv = "\
asset_key,day,price,volume
GC,2026-06-01,100,10
GC,2026-06-02,101,10
GC,2026-06-12,110,40
XX_GHOST,2026-06-12,1,1
XX_GHOST,2026-06-13,2,2
";
        let dir = write_temp(toml, &[("data/response_daily.csv", csv)]);
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let ghost_gaps: Vec<_> = result
            .boundary
            .missing
            .iter()
            .filter(|g| g.scope == "XX_GHOST")
            .collect();
        assert_eq!(ghost_gaps.len(), 1);
        let respond_txt = format_respond_summary(&result);
        assert!(respond_txt.contains("evidence boundary:"));
        assert!(respond_txt.contains("XX_GHOST"));
        assert!(
            respond_txt.contains("undeclared_response_asset/XX_GHOST"),
            "MissingKind Display must not use Debug Other(...), got:\n{respond_txt}"
        );
        assert!(!respond_txt.contains("Some("));
        assert!(!respond_txt.contains("Other("));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn respond_summary_keeps_author_declared_caveats() {
        let cfg = crate::load_project(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/gold_fixture"),
        )
        .unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let respond_txt = format_respond_summary(&result);
        assert!(
            respond_txt.contains("trader_intent/GC"),
            "declared TraderIntent must appear on respond, got:\n{respond_txt}"
        );
        assert!(
            respond_txt.contains("route_attribution/gold_fixture"),
            "declared RouteAttribution caveat must appear on respond, got:\n{respond_txt}"
        );
        assert!(result.boundary.missing.iter().any(|g| {
            g.source == GapSource::Declared && matches!(g.kind, MissingKind::TraderIntent)
        }));
    }
}
