use std::io;
use std::process::ExitCode;

use clap::Parser;

use ultimate_installer::cache::{LocalCache, ProviderResolutionWrite, RESOLVER_OPENAI};
use ultimate_installer::catalog::{self, CatalogEntry};
use ultimate_installer::context;
use ultimate_installer::executor::{self, ExecutionOutcome};
use ultimate_installer::openai::{OpenAiClient, ProviderDecision};
use ultimate_installer::parser::{self, LocalParseOutcome};
use ultimate_installer::planner;
use ultimate_installer::spec::InstallSpec;

/// Ultimate Installer — natural language software installer.
///
/// Resolves explicit or descriptive install requests against a verified tool catalog,
/// compiles a canonical spec for deterministic local SQLite caching, previews the exact
/// package-manager command, requires approval, and verifies binaries.
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

/// How the requested tools were resolved, used to decide whether to persist a
/// descriptive request mapping after a verified install.
struct ResolvedRequest {
    tools: Vec<&'static CatalogEntry>,
    provider_trimmed_request: Option<String>,
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

    // Best-effort local SQLite cache open: if unavailable, continue through the uncached path.
    let (mut cache, cache_open_err) = match LocalCache::open_default() {
        Ok(cache) => (Some(cache), None),
        Err(err) => (None, Some(err.to_string())),
    };

    let resolved = match resolve_requested_tools(&raw_request, cache.as_ref()).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };

    // Always probe the live machine and rebuild the plan fresh.
    let plan = match planner::build_plan(resolved.tools) {
        Ok(plan) => plan,
        Err(err) => {
            eprintln!("Error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let compiled_spec = InstallSpec::from_entries(
        catalog::CATALOG_REVISION,
        plan.platform,
        context::detect_arch(),
        &plan.requested,
    )
    .compile();

    let prior_verification = match (&cache, &plan.command) {
        (Some(local_cache), Some(cmd)) => local_cache
            .lookup_verified_command(&compiled_spec, cmd)
            .ok()
            .flatten(),
        _ => None,
    };

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    let outcome = match executor::execute_plan(
        &plan,
        cli.dry_run,
        prior_verification.as_ref(),
        &mut stdout,
        &mut stderr,
        &mut input,
    ) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!("I/O error: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Record to the local SQLite cache ONLY on explicit ExecutionOutcome::Succeeded { verified }.
    match outcome {
        ExecutionOutcome::Succeeded { verified } => {
            if let Some(cmd) = &plan.command {
                let provider_write = resolved.provider_trimmed_request.as_deref().map(|req| {
                    ProviderResolutionWrite {
                        resolver: RESOLVER_OPENAI,
                        catalog_revision: catalog::CATALOG_REVISION,
                        trimmed_request: req,
                        tool_ids: compiled_spec.spec.tool_ids.clone(),
                    }
                });

                match cache.as_mut() {
                    Some(local_cache) => {
                        if let Err(err) = local_cache.record_verified_install(
                            &compiled_spec,
                            cmd,
                            &verified,
                            provider_write.as_ref(),
                        ) {
                            eprintln!(
                                "Warning: install succeeded, but local verification history could not be saved ({err})."
                            );
                        }
                    }
                    None => {
                        let reason = cache_open_err
                            .as_deref()
                            .unwrap_or("local cache unavailable");
                        eprintln!(
                            "Warning: install succeeded, but local verification history could not be saved ({reason})."
                        );
                    }
                }
            }
            ExitCode::SUCCESS
        }
        ExecutionOutcome::DryRun | ExecutionOutcome::NothingToInstall => ExitCode::SUCCESS,
        ExecutionOutcome::Cancelled | ExecutionOutcome::PartialOrFailed { .. } => ExitCode::FAILURE,
    }
}

async fn resolve_requested_tools(
    raw_request: &str,
    cache: Option<&LocalCache>,
) -> Result<ResolvedRequest, ExitCode> {
    let supported_list: Vec<&str> = catalog::all_entries().iter().map(|e| e.id).collect();
    let supported_str = supported_list.join(", ");

    match parser::parse_local_request(raw_request) {
        LocalParseOutcome::ExplicitInstall(tools) => Ok(ResolvedRequest {
            tools,
            provider_trimmed_request: None,
        }),
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
            let trimmed = raw_request.trim();

            // Check local SQLite descriptive-request cache BEFORE constructing OpenAiClient
            // so a previously verified descriptive request works with OPENAI_API_KEY unset.
            if let Some(local_cache) = cache
                && let Ok(Some(cached_tools)) = local_cache.lookup_request_resolution(
                    RESOLVER_OPENAI,
                    catalog::CATALOG_REVISION,
                    trimmed,
                )
            {
                return Ok(ResolvedRequest {
                    tools: cached_tools,
                    provider_trimmed_request: Some(trimmed.to_string()),
                });
            }

            let client = match OpenAiClient::from_env() {
                Ok(client) => client,
                Err(err) => {
                    eprintln!("Error: {err}");
                    return Err(ExitCode::FAILURE);
                }
            };

            match client.resolve_intent(raw_request).await {
                Ok(ProviderDecision::Install(tools)) => Ok(ResolvedRequest {
                    tools,
                    provider_trimmed_request: Some(trimmed.to_string()),
                }),
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
