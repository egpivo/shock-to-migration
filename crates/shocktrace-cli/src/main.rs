//! shocktrace CLI — validate / respond / flows / analyze / compare.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use shocktrace::load_project;
use shocktrace::measure::{
    format_divergence_summary, format_shock_report_summary, load_asset_shock_report,
    load_divergence_summary,
};
use shocktrace::report::{
    analyze_project, compare_projects, flows_view, format_compare_table, format_flows_summary,
    format_respond_summary, format_summary, respond_view,
};
use shocktrace::AssetKey;

#[derive(Parser, Debug)]
#[command(name = "shocktrace")]
#[command(about = "Deterministic measurement of shock responses and directional flows")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Validate project.toml and inputs.
    Validate { project: PathBuf },
    /// Market-response section + related evidence boundary.
    Respond {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
    /// Directional-flow section + related evidence boundary.
    /// Exit 0 even when not_declared (structured absence).
    Flows {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
    /// Full AnalysisResult (response + route evidence + flow sections).
    Analyze {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    /// Compare evidence-ladder states across projects (no migration verdict).
    Compare {
        projects: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
    /// Reusable market measurement tools (shock score, divergence).
    /// Not causal inference; see .local/docs/MEASUREMENT_CONTRACT.md.
    Measure {
        #[command(subcommand)]
        command: MeasureCommands,
    },
}

#[derive(Subcommand, Debug)]
enum MeasureCommands {
    /// Event-day z-score + horizon returns + activity anomaly for one asset.
    Shock {
        project: PathBuf,
        #[arg(long)]
        asset: String,
        /// Defaults to the first `applies_to = ["response", ...]` window
        /// that isn't the baseline window.
        #[arg(long = "event-window")]
        event_window: Option<String>,
        /// Defaults to `[response].baseline_window` from project.toml.
        #[arg(long = "baseline-window")]
        baseline_window: Option<String>,
        /// Observed trading sessions after event start (not calendar days).
        #[arg(long, value_delimiter = ',', default_value = "1,3,5,20")]
        horizons: Vec<usize>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
    /// D_t = r_A - r_B on matched trading days, z-scored vs baseline.
    Divergence {
        project: PathBuf,
        #[arg(long = "asset-a")]
        asset_a: String,
        #[arg(long = "asset-b")]
        asset_b: String,
        #[arg(long = "event-window")]
        event_window: Option<String>,
        #[arg(long = "baseline-window")]
        baseline_window: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum OutputFormat {
    Summary,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn argv_command() -> String {
    env::args().collect::<Vec<_>>().join(" ")
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Validate { project } => {
            let cfg = load_project(&project)?;
            println!(
                "ok: project '{}' ({} assets, {} routes, {} windows)",
                cfg.project_id,
                cfg.assets.len(),
                cfg.routes.len(),
                cfg.windows.len()
            );
        }
        Commands::Respond { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            match format {
                OutputFormat::Summary => println!("{}", format_respond_summary(&result)),
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&respond_view(&result))?);
                }
            }
        }
        Commands::Flows { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            match format {
                OutputFormat::Summary => println!("{}", format_flows_summary(&result)),
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&flows_view(&result))?);
                }
            }
        }
        Commands::Analyze { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            match format {
                OutputFormat::Summary => println!("{}", format_summary(&result)),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
            }
        }
        Commands::Compare { projects, format } => {
            if projects.is_empty() {
                return Err("compare requires at least one project path".into());
            }
            let mut results = Vec::new();
            for path in &projects {
                let cfg = load_project(path)?;
                results.push(analyze_project(&cfg, &argv_command())?);
            }
            let rows = compare_projects(&results);
            match format {
                OutputFormat::Summary => println!("{}", format_compare_table(&rows)),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
            }
        }
        Commands::Measure { command } => run_measure(command)?,
    }
    Ok(())
}

fn run_measure(command: MeasureCommands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        MeasureCommands::Shock {
            project,
            asset,
            event_window,
            baseline_window,
            horizons,
            format,
        } => {
            let cfg = load_project(&project)?;
            let asset_key = AssetKey::new(asset.clone());
            let report = load_asset_shock_report(
                &cfg,
                &asset_key,
                event_window.as_deref(),
                baseline_window.as_deref(),
                &horizons,
            )?;
            match format {
                OutputFormat::Summary => {
                    println!("{}", format_shock_report_summary(&asset, &report))
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        MeasureCommands::Divergence {
            project,
            asset_a,
            asset_b,
            event_window,
            baseline_window,
            format,
        } => {
            let cfg = load_project(&project)?;
            let summary = load_divergence_summary(
                &cfg,
                &AssetKey::new(asset_a),
                &AssetKey::new(asset_b),
                event_window.as_deref(),
                baseline_window.as_deref(),
            )?;
            match format {
                OutputFormat::Summary => println!("{}", format_divergence_summary(&summary)),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
            }
        }
    }
    Ok(())
}
