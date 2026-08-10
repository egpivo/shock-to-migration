//! Emit markdown evidence-ladder table from real project `ladder_status` rows.
use shocktrace::{analyze_project, compare_projects, format_compare_table, load_project};
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join(".local/docs/EVIDENCE_LADDER.md");
    let mut results = Vec::new();
    for id in ["spacex", "gold", "oil"] {
        let cfg = load_project(root.join(format!("projects/{id}"))).expect("load project");
        results.push(analyze_project(&cfg, "write_ladder").expect("analyze"));
    }
    let rows = compare_projects(&results);
    let mut md = String::new();
    md.push_str("# Evidence ladder comparison\n\n");
    md.push_str(
        "Generated from engine `compare_projects` / `ladder_status` over `projects/{spacex,gold,oil}`. Gold/Oil samples are frozen architecture inputs, not historical market claims.\n\n",
    );
    md.push_str(&format_compare_table(&rows));
    md.push('\n');
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("create .local/docs");
    }
    std::fs::write(&out, md).expect("write .local/docs/EVIDENCE_LADDER.md");
    println!("wrote {}", out.display());
}
