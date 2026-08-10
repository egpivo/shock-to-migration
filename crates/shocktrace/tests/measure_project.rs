//! Integration tests: `measure` project-loading path (load_project ->
//! response CSV ingest -> account_* functions), exercising window
//! resolution and error surfaces that unit tests inside `measure/*.rs`
//! can't reach (those test the pure accounting functions directly).

use std::fs;
use std::path::PathBuf;

use shocktrace::measure::{
    load_asset_shock_report, load_divergence_summary, load_response_gap_summary,
    MeasureProjectError,
};
use shocktrace::{load_project, AssetKey, MeasureError};

/// Fixture: A daily returns 0.02, 0.04, 0.02, 0.04 (baseline), 0.10 (event).
/// B daily returns 0.01 x4 (baseline), 0.01 (event).
/// D_t (A-B) baseline = 0.01, 0.03, 0.01, 0.03 -> mean 0.02, std 0.01.
/// Event D = 0.09 -> z = 7. Asset A alone: baseline mean 0.03, std 0.01,
/// event 0.10 -> z = 7.
const PROJECT_TOML: &str = r#"
schema_version = 3
project_id = "measure_it_fixture"
name = "Measure integration fixture"

[event]
id = "measure_it_event"
name = "Measure integration test event"
timestamp = "2026-01-06T00:00:00Z"

[[windows]]
name = "baseline"
start = "2026-01-02"
end = "2026-01-05"

[[windows]]
name = "event"
start = "2026-01-06"
end = "2026-01-06"

[[assets]]
key = "A"
chain = "test-chain"
opaque_id = "a-id"
display_symbol = "A"
product_kind = "unknown"

[[assets]]
key = "B"
chain = "test-chain"
opaque_id = "b-id"
display_symbol = "B"
product_kind = "unknown"

[response]
baseline_window = "baseline"

[inputs]
response = "response.csv"
references = "references.csv"
"#;

const RESPONSE_CSV: &str = "asset_key,day,price,volume\n\
A,2026-01-01,100,\n\
A,2026-01-02,102,\n\
A,2026-01-03,106.08,\n\
A,2026-01-04,108.2016,\n\
A,2026-01-05,112.529664,\n\
A,2026-01-06,123.7826304,\n\
B,2026-01-01,50,\n\
B,2026-01-02,50.5,\n\
B,2026-01-03,51.005,\n\
B,2026-01-04,51.51505,\n\
B,2026-01-05,52.0302005,\n\
B,2026-01-06,52.550502505,\n";

const REFERENCE_CSV: &str =
    "reference_key,asset_key,day,reference_return,source_label,source_url,cutoff\n\
A_REF,A,2026-01-06,0.08,fixture,https://example.test,fixture close\n";

fn write_fixture_project() -> PathBuf {
    let dir = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "shocktrace-measure-{}-{:?}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
            n
        ))
    };
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("project.toml"), PROJECT_TOML).unwrap();
    fs::write(dir.join("response.csv"), RESPONSE_CSV).unwrap();
    fs::write(dir.join("references.csv"), REFERENCE_CSV).unwrap();
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn shock_report_uses_default_event_and_baseline_windows() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let report =
        load_asset_shock_report(&cfg, &AssetKey::new("A"), None, None, &[1, 3, 5, 20]).unwrap();

    assert_eq!(report.shock.event_window_name, "event");
    assert_eq!(report.shock.baseline_window_name, "baseline");
    assert_eq!(report.shock.baseline_n, 4);
    assert_eq!(report.shock.baseline_mean, Some("0.03".parse().unwrap()));
    assert_eq!(report.shock.baseline_std, Some("0.01".parse().unwrap()));
    assert_eq!(report.shock.event_return, Some("0.10".parse().unwrap()));
    assert_eq!(report.shock.z_score, Some("7".parse().unwrap()));

    // Anchor is the event window's start day (2026-01-06), which is also
    // the last observed day in the fixture, so every horizon is None.
    assert!(report.horizons.horizons.iter().all(|h| h.day.is_none()));

    cleanup(&dir);
}

#[test]
fn divergence_report_known_fixture_via_project_loader() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let summary =
        load_divergence_summary(&cfg, &AssetKey::new("A"), &AssetKey::new("B"), None, None)
            .unwrap();

    assert_eq!(summary.matched_days, 5);
    assert_eq!(summary.baseline_n, 4);
    assert_eq!(summary.baseline_mean, Some("0.02".parse().unwrap()));
    assert_eq!(summary.baseline_std, Some("0.01".parse().unwrap()));
    assert_eq!(summary.event_divergence, Some("0.09".parse().unwrap()));
    assert_eq!(summary.z_score, Some("7".parse().unwrap()));

    cleanup(&dir);
}

#[test]
fn response_gap_report_known_fixture_via_project_loader() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let summary = load_response_gap_summary(&cfg, &AssetKey::new("A"), "A_REF", None).unwrap();

    assert_eq!(summary.reference_return, "0.08".parse().unwrap());
    assert_eq!(summary.token_return, Some("0.10".parse().unwrap()));
    assert_eq!(summary.response_gap, Some("0.02".parse().unwrap()));
    assert_eq!(summary.direction_match, Some(true));

    cleanup(&dir);
}

#[test]
fn explicit_window_overrides_are_honored() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let report = load_asset_shock_report(
        &cfg,
        &AssetKey::new("A"),
        Some("event"),
        Some("baseline"),
        &[1],
    )
    .unwrap();
    assert_eq!(report.shock.z_score, Some("7".parse().unwrap()));

    cleanup(&dir);
}

#[test]
fn unknown_window_override_is_a_structured_error() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let err = load_asset_shock_report(
        &cfg,
        &AssetKey::new("A"),
        Some("does-not-exist"),
        None,
        &[1],
    )
    .unwrap_err();
    assert!(matches!(err, MeasureProjectError::WindowNotFound(_)));

    cleanup(&dir);
}

#[test]
fn unknown_asset_is_a_structured_error_not_a_panic() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let err = load_asset_shock_report(&cfg, &AssetKey::new("ZZZ"), None, None, &[1]).unwrap_err();
    assert!(matches!(err, MeasureProjectError::UndeclaredAsset { .. }));

    cleanup(&dir);
}

#[test]
fn divergence_with_unknown_second_asset_errors() {
    let dir = write_fixture_project();
    let cfg = load_project(&dir).unwrap();

    let err = load_divergence_summary(&cfg, &AssetKey::new("A"), &AssetKey::new("ZZZ"), None, None)
        .unwrap_err();
    assert!(matches!(err, MeasureProjectError::UndeclaredAsset { .. }));

    cleanup(&dir);
}

#[test]
fn measure_includes_declared_caveats_and_low_baseline_gap() {
    let dir = write_fixture_project();
    // Append a declared caveat to the fixture project.
    let toml = fs::read_to_string(dir.join("project.toml")).unwrap()
        + r#"

[[coverage_declared]]
kind = "historical_depth"
scope = "A"
reason = "fixture: only daily closes"
"#;
    fs::write(dir.join("project.toml"), toml).unwrap();
    let cfg = load_project(&dir).unwrap();

    let report = load_asset_shock_report(&cfg, &AssetKey::new("A"), None, None, &[1]).unwrap();

    assert!(report.shock.low_baseline);
    assert!(
        report
            .boundary
            .missing
            .iter()
            .any(|g| g.kind.to_string() == "historical_depth" && g.scope == "A"),
        "declared caveat must appear on measure boundary: {:?}",
        report.boundary.missing
    );
    assert!(
        report
            .boundary
            .missing
            .iter()
            .any(|g| g.kind.to_string() == "low_baseline"),
        "thin baseline must emit low_baseline gap: {:?}",
        report.boundary.missing
    );

    cleanup(&dir);
}

#[test]
fn measure_error_reexported_at_crate_root() {
    // Sanity check on the lib.rs export surface this subagent owns.
    let err = shocktrace::daily_returns(vec![]).unwrap_err();
    assert!(matches!(err, MeasureError::EmptySeries(_)));
}
