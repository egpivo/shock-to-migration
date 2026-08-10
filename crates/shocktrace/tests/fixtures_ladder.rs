//! Integration tests: gold/oil response-only vs linked synthetic conduit.

use shocktrace::evidence::EvidenceSection;
use shocktrace::report::{analyze_project, ladder_status};
use shocktrace::{load_project, AssetLocator};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn gold_fixture_validates_and_analyzes_response_only() {
    let cfg = load_project(root().join("tests/gold_fixture")).unwrap();
    assert!(cfg.routes.is_empty());
    assert!(matches!(
        cfg.assets[0].id.locator,
        AssetLocator::MarketInstrument { .. }
    ));
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
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("is_migration"));
}

#[test]
fn oil_fixture_requires_no_core_special_case() {
    let cfg = load_project(root().join("tests/oil_fixture")).unwrap();
    let result = analyze_project(&cfg, "test").unwrap();
    assert!(matches!(
        result.directional_flow,
        EvidenceSection::NotDeclared { .. }
    ));
    // Same engine path as gold — project_id is data, not a branch.
    assert_eq!(result.project_id, "oil_fixture");
}

#[test]
fn linked_synthetic_still_exposes_flow() {
    let cfg = load_project(root().join("tests/synthetic_conduit")).unwrap();
    let result = analyze_project(&cfg, "test").unwrap();
    assert!(matches!(
        result.directional_flow,
        EvidenceSection::Available { .. }
    ));
    assert!(matches!(
        result.route_evidence,
        EvidenceSection::Available { .. }
    ));
}

#[test]
fn ladder_comparison_rows() {
    let cases = ["synthetic_conduit", "gold_fixture", "oil_fixture"];
    let mut rows = Vec::new();
    for id in cases {
        let cfg = load_project(root().join(format!("tests/{id}"))).unwrap();
        let result = analyze_project(&cfg, "test").unwrap();
        rows.push(ladder_status(&result));
    }
    assert_eq!(rows[0].directional_flow, "observed");
    assert_eq!(rows[1].directional_flow, "not_declared");
    assert_eq!(rows[2].directional_flow, "not_declared");
    assert_eq!(rows[1].market_response, "observed");
    assert_eq!(rows[2].market_response, "observed");
}
