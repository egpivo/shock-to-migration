//! Emit markdown evidence-ladder table from fixture `ladder_status` rows.
use shocktrace::{analyze_project, ladder_status, load_project};
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join(".local/docs/EVIDENCE_LADDER.md");
    let cases = [
        (
            "synthetic_conduit (linked-route / SpaceX-class observability)",
            "synthetic_conduit",
        ),
        ("gold_fixture", "gold_fixture"),
        ("oil_fixture", "oil_fixture"),
    ];
    let mut md = String::new();
    md.push_str("# Evidence ladder comparison\n\n");
    md.push_str(
        "Generated from engine `ladder_status` over fixture projects. Fixtures are architecture probes, not historical claims.\n\n",
    );
    md.push_str(
        "| Case | Market response | Route evidence | Directional flow | What can be claimed |\n",
    );
    md.push_str("|---|---|---|---|---|\n");
    for (label, id) in cases {
        let cfg = load_project(root.join(format!("tests/{id}"))).expect("load fixture");
        let result = analyze_project(&cfg, "write_ladder").expect("analyze");
        let row = ladder_status(&result);
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            label,
            row.market_response,
            row.route_evidence,
            row.directional_flow,
            row.claim_boundary
        ));
    }
    md.push_str(
        "\nReading: different markets expose different parts of the shock-to-migration chain. The engine stops where the evidence stops. `not_declared` is not `flow = 0`, and it is not `is_migration = false`.\n",
    );
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("create .local/docs");
    }
    std::fs::write(&out, md).expect("write .local/docs/EVIDENCE_LADDER.md");
    println!("wrote {}", out.display());
}
