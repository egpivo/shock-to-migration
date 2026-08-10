//! Assemble machine-readable analysis results.
//!
//! Report generation prints numbers and evidence boundaries.
//! It does not emit migration verdicts.
//!
//! Each route uses its own `measurement` config for unit, attribution, and denominator.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::Utc;
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

use crate::coverage::{CoverageGap, EvidenceBoundary, MissingKind};
use crate::flow::{
    account_directional_flows, inclusive_day_count, DirectionalFlowObservation, FlowSeriesSummary,
    FlowUnit,
};
use crate::ingest::daily_flows::load_daily_flow_rows;
use crate::ingest::daily_supply::{load_daily_supply, supply_on};
use crate::project::{ProjectConfig, ProjectError};
use crate::provenance::{hash_bytes, hash_file, InputHash, ProvenanceRecord};
use crate::route::{DenominatorPolicy, FlowUnitConfig, Route, RouteEvidence};

const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const METRIC_DEFINITION_ID: &str = "directional_flow.v2";

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Flow(#[from] crate::flow::FlowError),
    #[error("ingest error: {0}")]
    Ingest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub project_id: String,
    pub engine_version: String,
    pub route_evidence: Vec<RouteEvidence>,
    pub flow_summaries: Vec<FlowSeriesSummary>,
    pub boundary: EvidenceBoundary,
    pub provenance: ProvenanceRecord,
}

pub fn analyze_project(cfg: &ProjectConfig, command: &str) -> Result<AnalysisResult, AnalyzeError> {
    let mut boundary = EvidenceBoundary::default();
    boundary.merge_declared(cfg.coverage_declared.clone());
    boundary.assume(
        "Directional flow summaries are accounting objects, not migration classifications.",
    );

    let flows_path = cfg.root.join(&cfg.inputs.flows);
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

    // Load supply once if any route needs it.
    let supply_rows = if cfg
        .routes
        .iter()
        .any(|r| r.measurement.denominator.is_some())
    {
        match &cfg.inputs.supply {
            Some(rel) => {
                let path = cfg.root.join(rel);
                Some(load_daily_supply(&path).map_err(|e| AnalyzeError::Ingest(e.to_string()))?)
            }
            None => None,
        }
    } else {
        None
    };

    let mut flow_summaries = Vec::new();

    for route in &cfg.routes {
        let denominator = resolve_route_denominator(route, supply_rows.as_deref(), &mut boundary)?;

        let Some(series) = by_route.remove(&route.id) else {
            for window in &cfg.windows {
                boundary.push_detected(CoverageGap {
                    kind: MissingKind::RouteAttribution,
                    scope: format!("{}@{}", route.id, window.name),
                    reason: format!(
                        "no flow observations found for route in window '{}' (0 of {} days)",
                        window.name,
                        inclusive_day_count(window)
                    ),
                });
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
                boundary.push_detected(CoverageGap {
                    kind: MissingKind::RouteAttribution,
                    scope: format!("{}@{}", route.id, window.name),
                    reason: format!(
                        "no flow observations in window '{}' (0 of {window_days} days)",
                        window.name
                    ),
                });
                continue;
            }

            let summary = account_directional_flows(in_window, window, denominator)?;
            if summary.observed_days < summary.window_days {
                boundary.push_detected(CoverageGap {
                    kind: MissingKind::RouteAttribution,
                    scope: format!("{}@{}", route.id, window.name),
                    reason: format!(
                        "{} of {} days missing in window '{}'",
                        summary.window_days - summary.observed_days,
                        summary.window_days,
                        window.name
                    ),
                });
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
            boundary.push_detected(CoverageGap {
                kind: MissingKind::RouteAttribution,
                scope: route.id.clone(),
                reason: format!(
                    "{} observation day(s) outside all declared windows (e.g. {})",
                    outside_days.len(),
                    sample.join(", ")
                ),
            });
        }
    }

    for extra_route in undeclared_routes {
        boundary.push_detected(CoverageGap {
            kind: MissingKind::RouteAttribution,
            scope: extra_route,
            reason: "flow observations reference a route_id not declared in project.toml".into(),
        });
    }

    // Any leftover declared-route buckets should already have been consumed.
    debug_assert!(by_route.is_empty());

    let provenance = build_provenance(cfg, command)?;

    Ok(AnalysisResult {
        project_id: cfg.project_id.clone(),
        engine_version: ENGINE_VERSION.to_string(),
        route_evidence: cfg.route_evidence.clone(),
        flow_summaries,
        boundary,
        provenance,
    })
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
        boundary.push_detected(CoverageGap {
            kind: MissingKind::Supply,
            scope: format!("{}:{}", route.id, asset),
            reason: "denominator requested but inputs.supply missing".into(),
        });
        return Ok(None);
    };

    match supply_on(rows, asset, *as_of) {
        Ok(value) => Ok(Some(value)),
        Err(e) => {
            boundary.push_detected(CoverageGap {
                kind: MissingKind::Supply,
                scope: format!("{}:{}", route.id, asset),
                reason: e.to_string(),
            });
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

    let flows_rel = cfg.inputs.flows.clone();
    input_hashes.push(InputHash {
        path: flows_rel.clone(),
        sha256: hash_file(Path::new(&cfg.root.join(&flows_rel)))?,
    });

    if let Some(supply) = &cfg.inputs.supply {
        input_hashes.push(InputHash {
            path: supply.clone(),
            sha256: hash_file(Path::new(&cfg.root.join(supply)))?,
        });
    }

    Ok(ProvenanceRecord {
        engine_version: ENGINE_VERSION.to_string(),
        project_id: cfg.project_id.clone(),
        config_sha256: hash_bytes(&config_bytes),
        input_hashes,
        metric_definition_id: METRIC_DEFINITION_ID.to_string(),
        command: command.to_string(),
        computed_at_unix: Utc::now().timestamp(),
    })
}

/// Human-readable terminal summary. Numbers + gaps only.
pub fn format_summary(result: &AnalysisResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("project: {}", result.project_id));
    lines.push(format!("engine: {}", result.engine_version));
    lines.push(format!(
        "metric: {}",
        result.provenance.metric_definition_id
    ));
    lines.push(String::new());

    for s in &result.flow_summaries {
        lines.push(format!("route {} @ window {}", s.route_id, s.window_name));
        lines.push(format!(
            "  observed/window:   {}/{}",
            s.observed_days, s.window_days
        ));
        lines.push(format!("  unit:              {:?}", s.unit));
        lines.push(format!("  attribution:       {:?}", s.attribution));
        lines.push(format!("  gross A→B:         {}", s.gross_a_to_b_total));
        lines.push(format!("  gross B→A:         {}", s.gross_b_to_a_total));
        lines.push(format!("  net:               {}", s.net_total));
        lines.push(format!("  peak cum net:      {}", s.peak_cumulative_net));
        lines.push(format!("  trough cum net:    {}", s.trough_cumulative_net));
        lines.push(format!(
            "  days cum negative: {}",
            s.days_cumulative_negative
        ));
        lines.push(format!(
            "  reversal_ratio:    {}",
            opt_dec(s.reversal_ratio)
        ));
        lines.push(format!(
            "  net/denominator:   {}",
            opt_dec(s.net_over_denominator)
        ));
        lines.push(format!("  sign_change_days:  {}", s.sign_change_days));
        lines.push(format!("  note: {}", s.interpretation_boundary));
        lines.push(String::new());
    }

    lines.push("evidence boundary:".into());
    if result.boundary.missing.is_empty() {
        lines.push("  (no coverage gaps recorded)".into());
    } else {
        for gap in &result.boundary.missing {
            lines.push(format!("  - {:?}/{}: {}", gap.kind, gap.scope, gap.reason));
        }
    }
    for a in &result.boundary.assumptions {
        lines.push(format!("  assumption: {a}"));
    }

    lines.join("\n")
}

fn opt_dec(v: Option<Decimal>) -> String {
    match v {
        Some(d) => d.to_string(),
        None => "null".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use std::{fs, path::PathBuf};

    fn write_temp_project(project_toml: &str, flows_csv: &str, supply_csv: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shocktrace-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(dir.join("data/flows_daily.csv"), flows_csv).unwrap();
        fs::write(dir.join("data/supply_daily.csv"), supply_csv).unwrap();
        fs::write(dir.join("project.toml"), project_toml).unwrap();
        dir
    }

    fn single_route_toml(denom_asset: &str, unit_asset: &str, measured_leg: &str) -> String {
        format!(
            r#"
schema_version = 2
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
    fn partial_window_coverage_emits_gap() {
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2026-06-12,100.0,20.0
a_b_swap,2026-06-13,10.0,0.0
";
        let supply = "\
asset_key,day,supply
A,2026-06-11,1000.0
";
        let dir = write_temp_project(&single_route_toml("A", "A", "source"), flows, supply);
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let post = result
            .flow_summaries
            .iter()
            .find(|s| s.window_name == "post_event")
            .unwrap();
        assert_eq!(post.observed_days, 2);
        assert_eq!(post.window_days, 7);
        assert_eq!(post.net_total, Decimal::from_str("90").unwrap());
        assert!(result.boundary.missing.iter().any(|g| {
            g.scope == "a_b_swap@post_event" && g.reason.contains("5 of 7 days missing")
        }));
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
        let supply = "\
asset_key,day,supply
A,2026-06-11,1000.0
";
        let dir = write_temp_project(&single_route_toml("A", "A", "source"), flows, supply);
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let post = result
            .flow_summaries
            .iter()
            .find(|s| s.window_name == "post_event")
            .unwrap();
        assert_eq!(post.observed_days, 1);
        assert_eq!(post.net_total, Decimal::from(9));
        assert!(result
            .boundary
            .missing
            .iter()
            .any(|g| g.reason.contains("outside all declared windows")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_mismatched_denominator_at_validate() {
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2026-06-12,10.0,1.0
";
        let supply = "\
asset_key,day,supply
A,2026-06-11,1000.0
B,2026-06-11,5000.0
";
        let dir = write_temp_project(&single_route_toml("B", "A", "source"), flows, supply);
        let err = crate::load_project(&dir).unwrap_err();
        assert!(err
            .to_string()
            .contains("must equal measurement.unit_asset"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unit_asset_not_matching_measured_leg() {
        let flows = "\
route_id,day,gross_a_to_b,gross_b_to_a
a_b_swap,2026-06-12,10.0,1.0
";
        let supply = "\
asset_key,day,supply
A,2026-06-11,1000.0
B,2026-06-11,5000.0
";
        // measured_leg=source(A) but unit_asset=B
        let dir = write_temp_project(&single_route_toml("B", "B", "source"), flows, supply);
        let err = crate::load_project(&dir).unwrap_err();
        assert!(err.to_string().contains("must equal measured_leg asset"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn multi_route_independent_units_and_denominators() {
        // A→B measured in A / supply 1000; C→B measured in C / supply 500.
        // Nets must scale independently: 30/1000=0.03 and 40/500=0.08.
        let toml = r#"
schema_version = 2
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
        let supply = "\
asset_key,day,supply
A,2026-06-11,1000.0
C,2026-06-11,500.0
";
        let dir = write_temp_project(toml, flows, supply);
        let cfg = crate::load_project(&dir).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        assert_eq!(result.flow_summaries.len(), 2);

        let ab = result
            .flow_summaries
            .iter()
            .find(|s| s.route_id == "a_b_swap")
            .unwrap();
        assert_eq!(ab.net_total, Decimal::from(30));
        assert_eq!(
            ab.net_over_denominator.unwrap(),
            Decimal::from_str("0.03").unwrap()
        );
        assert_eq!(
            ab.unit,
            FlowUnit::TokenNative {
                asset: crate::identity::AssetKey::new("A")
            }
        );

        let cb = result
            .flow_summaries
            .iter()
            .find(|s| s.route_id == "c_b_swap")
            .unwrap();
        assert_eq!(cb.net_total, Decimal::from(40));
        assert_eq!(
            cb.net_over_denominator.unwrap(),
            Decimal::from_str("0.08").unwrap()
        );
        assert_eq!(
            cb.unit,
            FlowUnit::TokenNative {
                asset: crate::identity::AssetKey::new("C")
            }
        );

        let _ = fs::remove_dir_all(dir);
    }
}
