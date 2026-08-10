//! shocktrace CLI — validate projects and compute directional flow accounting.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use shocktrace::load_project;
use shocktrace::report::{analyze_project, format_summary};

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
    /// Validate project.toml, identity uniqueness, windows, routes, and input paths.
    Validate {
        /// Path to a project directory containing project.toml
        project: PathBuf,
    },
    /// Compute directional flow accounting for declared routes × windows.
    Flows {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
        format: OutputFormat,
    },
    /// Validate + flows; emit AnalysisResult (JSON) and optional summary.
    Analyze {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
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
            // load_project already validates.
            let cfg = load_project(&project)?;
            println!(
                "ok: project '{}' ({} assets, {} routes, {} windows)",
                cfg.project_id,
                cfg.assets.len(),
                cfg.routes.len(),
                cfg.windows.len()
            );
        }
        Commands::Flows { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            emit(&result, format)?;
        }
        Commands::Analyze { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            emit(&result, format)?;
        }
    }
    Ok(())
}

fn emit(
    result: &shocktrace::report::AnalysisResult,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Summary => {
            println!("{}", format_summary(result));
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
    }
    Ok(())
}
