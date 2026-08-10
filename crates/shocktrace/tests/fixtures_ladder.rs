//! Integration: real projects under projects/ + fixtures ladder.

use rust_decimal::Decimal;
use shocktrace::evidence::EvidenceSection;
use shocktrace::report::{analyze_project, compare_projects, format_compare_table, ladder_status};
use shocktrace::{load_project, AnalysisSection, GapSource};
use std::path::PathBuf;
use std::str::FromStr;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn gold_project_response_only_no_zero_flow() {
    let cfg = load_project(root().join("projects/gold")).unwrap();
    assert!(cfg.routes.is_empty());
    let result = analyze_project(&cfg, "test").unwrap();
    assert!(matches!(
        result.market_response,
        EvidenceSection::Available { .. }
    ));
    assert!(matches!(
        result.directional_flow,
        EvidenceSection::NotDeclared { .. }
    ));
    if let EvidenceSection::NotDeclared { reason } = &result.directional_flow {
        assert!(reason.contains("not zero"));
    }
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("is_migration"));
}

#[test]
fn oil_project_requires_no_core_special_case() {
    let cfg = load_project(root().join("projects/oil")).unwrap();
    let result = analyze_project(&cfg, "test").unwrap();
    assert_eq!(result.project_id, "oil");
    assert!(matches!(
        result.directional_flow,
        EvidenceSection::NotDeclared { .. }
    ));
}

#[test]
fn spacex_full_flow_period_matches_frozen_headline() {
    let cfg = load_project(root().join("projects/spacex")).unwrap();
    let result = analyze_project(&cfg, "test").unwrap();
    let EvidenceSection::Available { data } = &result.directional_flow else {
        panic!("expected flows");
    };
    assert_eq!(
        data.len(),
        1,
        "only flow-applicable window should produce a summary"
    );
    let full = data
        .iter()
        .find(|s| s.window_name == "full_flow_period")
        .expect("full_flow_period");
    assert_eq!(
        full.gross_a_to_b_total.as_decimal(),
        Decimal::from_str("1540.0422").unwrap()
    );
    assert_eq!(
        full.gross_b_to_a_total.as_decimal(),
        Decimal::from_str("1275.9703").unwrap()
    );
    assert_eq!(full.net_total, Decimal::from_str("264.0719").unwrap());
    assert_eq!(
        full.peak_cumulative_net,
        Decimal::from_str("444.2477").unwrap()
    );
    let denom_share = full.net_over_denominator.unwrap();
    assert!(
        (denom_share - Decimal::from_str("0.030205").unwrap()).abs()
            < Decimal::from_str("0.000001").unwrap(),
        "net/denom got {denom_share}"
    );
    let peak_share = full.peak_cumulative_net / Decimal::from_str("8742.556").unwrap();
    assert!(
        (peak_share - Decimal::from_str("0.050814").unwrap()).abs()
            < Decimal::from_str("0.000001").unwrap(),
        "peak/denom got {peak_share}"
    );
    assert_eq!(full.observed_days, 43);
    assert_eq!(full.window_days, 56);
    // Sparse observation points with cum < 0 (not article's incorrect "six days").
    assert_eq!(full.observations_cumulative_negative, 4);
    // Response-only baseline must not create a flow coverage gap for baseline.
    assert!(!result
        .boundary
        .missing
        .iter()
        .any(|g| { g.section == AnalysisSection::Flow && g.scope.contains("@baseline") }));
}

#[test]
fn compare_three_real_projects_from_engine_states() {
    let mut results = Vec::new();
    for id in ["spacex", "gold", "oil"] {
        let cfg = load_project(root().join(format!("projects/{id}"))).unwrap();
        results.push(analyze_project(&cfg, "compare").unwrap());
    }
    let rows = compare_projects(&results);
    assert_eq!(rows.len(), 3);
    let spacex = rows.iter().find(|r| r.project_id == "spacex").unwrap();
    let gold = rows.iter().find(|r| r.project_id == "gold").unwrap();
    assert_eq!(spacex.directional_flow.status, "observed");
    assert!(!spacex.directional_flow.coverage.is_empty());
    assert_eq!(spacex.directional_flow.coverage[0].observed_days, 43);
    assert_eq!(spacex.directional_flow.coverage[0].window_days, 56);
    assert_eq!(gold.directional_flow.status, "not_declared");
    assert_eq!(gold.route_evidence.status, "not_declared");
    let table = format_compare_table(&rows);
    assert!(table.contains("43/56"));
    assert!(table.contains("not_declared"));
}

#[test]
fn fixture_ladder_still_works() {
    let cases = ["synthetic_conduit", "gold_fixture", "oil_fixture"];
    for id in cases {
        let cfg = load_project(root().join(format!("tests/{id}"))).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        let row = ladder_status(&result);
        assert!(!row.project_id.is_empty());
    }
}

#[test]
fn detected_gaps_carry_analysis_section() {
    let cfg = load_project(root().join("projects/spacex")).unwrap();
    let result = analyze_project(&cfg, "test").unwrap();
    let detected: Vec<_> = result
        .boundary
        .missing
        .iter()
        .filter(|g| g.source == GapSource::Detected)
        .collect();
    assert!(!detected.is_empty());
    for g in &detected {
        assert!(matches!(
            g.section,
            AnalysisSection::Response | AnalysisSection::Flow | AnalysisSection::General
        ));
    }
    assert!(detected.iter().any(|g| {
        g.section == AnalysisSection::Flow && g.reason.contains("13 of 56 days missing")
    }));
}
