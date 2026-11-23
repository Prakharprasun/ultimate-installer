use std::io;
use std::process::ExitCode;

use clap::Parser;

use ultimate_installer::catalog::{self, CatalogEntry};
use ultimate_installer::executor;
use ultimate_installer::openai::{OpenAiClient, ProviderDecision};
use ultimate_installer::parser::{self, LocalParseOutcome};
use ultimate_installer::planner;

/// Ultimate Installer — natural language software installer.
///
/// Resolves explicit or descriptive install requests against a verified tool catalog,
/// previews the exact package-manager command, requires approval, and verifies binaries.
#[derive(Parser, Debug)]
#[command(name = "ulti", version, about, long_about = None)]
struct Cli {
    /// The install request, e.g. "install ripgrep and fd"
    #[arg()]
    command: Option<String>,

    /// Run the first-time configuration wizard
    #[arg(long)]
    config: bool,

    /// Show the installation plan and exact command without installing anything
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.config {
        println!("🔧 ulti config — interactive wizard is on the roadmap.");
        println!("   For descriptive requests, set OPENAI_API_KEY in your environment.");
        println!(
            "   Explicit requests (e.g. `ulti \"install ripgrep and fd\"`) work without a key."
        );
        return ExitCode::SUCCESS;
    }

    let Some(raw_request) = cli.command else {
        eprintln!("⚠ No command provided.\n");
        eprintln!("Usage:  ulti \"install ripgrep and fd\"");
        eprintln!("        ulti \"install ripgrep and fd\" --dry-run");
        eprintln!("        ulti --config\n");
        eprintln!("Run `ulti --help` for more options.");
        return ExitCode::FAILURE;
    };

    let tools = match resolve_requested_tools(&raw_request).await {
        Ok(tools) => tools,
        Err(code) => return code,
    };

    let plan = match planner::build_plan(tools) {
        Ok(plan) => plan,
        Err(err) => {
            eprintln!("Error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    match executor::execute_plan(&plan, cli.dry_run, &mut stdout, &mut stderr, &mut input) {
        Ok(outcome) if outcome.is_success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("I/O error: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn resolve_requested_tools(
    raw_request: &str,
) -> Result<Vec<&'static CatalogEntry>, ExitCode> {
    let supported_list: Vec<&str> = catalog::all_entries().iter().map(|e| e.id).collect();
    let supported_str = supported_list.join(", ");

    match parser::parse_local_request(raw_request) {
        LocalParseOutcome::ExplicitInstall(tools) => Ok(tools),
        LocalParseOutcome::UnsupportedExplicit { unknown } => {
            eprintln!(
                "Error: Unsupported package(s): {}. No changes were made.",
                unknown.join(", ")
            );
            eprintln!("Supported tools: {supported_str}");
            Err(ExitCode::FAILURE)
        }
        LocalParseOutcome::InvalidRequest(msg) => {
            eprintln!("Error: {msg}");
            Err(ExitCode::FAILURE)
        }
        LocalParseOutcome::NeedsProvider => {
            let client = match OpenAiClient::from_env() {
                Ok(client) => client,
                Err(err) => {
                    eprintln!("Error: {err}");
                    return Err(ExitCode::FAILURE);
                }
            };

            match client.resolve_intent(raw_request).await {
                Ok(ProviderDecision::Install(tools)) => Ok(tools),
                Ok(ProviderDecision::Unsupported { reason }) => {
                    eprintln!("Unsupported request: {reason}");
                    eprintln!("Supported tools: {supported_str}");
                    eprintln!("No changes were made.");
                    Err(ExitCode::FAILURE)
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                    Err(ExitCode::FAILURE)
                }
            }
        }
    }
}
