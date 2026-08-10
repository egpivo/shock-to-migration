//! shocktrace CLI — validate / respond / flows / analyze.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use shocktrace::load_project;
use shocktrace::report::{
    analyze_project, flows_view, format_flows_summary, format_respond_summary, format_summary,
    respond_view,
};

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
            // Exit 0: not_declared / not_observable are successful structured answers.
        }
        Commands::Analyze { project, format } => {
            let cfg = load_project(&project)?;
            let result = analyze_project(&cfg, &argv_command())?;
            match format {
                OutputFormat::Summary => println!("{}", format_summary(&result)),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
            }
        }
    }
    Ok(())
}
