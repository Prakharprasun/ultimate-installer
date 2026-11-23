//! Execution engine — previews plans, collects explicit user approval,
//! runs the exact command vector safely via `std::process::Command`, and verifies installed tools.

use std::io::{self, BufRead, Write};
use std::process::{Command, ExitStatus};

use crate::auth;
use crate::catalog::CatalogEntry;
use crate::planner::{self, InstallCommand, InstallPlan, InstalledTool};

/// Result of executing (or previewing) an install plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// All requested tools were already installed; nothing was prompted or executed.
    NothingToInstall,
    /// `--dry-run` printed the plan without modifying the machine.
    DryRun,
    /// User declined, submitted empty input, or hit EOF.
    Cancelled,
    /// Package manager succeeded and all requested tools verified.
    Succeeded { verified: Vec<InstalledTool> },
    /// Package manager failed or one or more tools failed verification.
    PartialOrFailed {
        verified: Vec<InstalledTool>,
        failed: Vec<&'static CatalogEntry>,
        command_exit_code: Option<i32>,
    },
}

impl ExecutionOutcome {
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            ExecutionOutcome::NothingToInstall
                | ExecutionOutcome::DryRun
                | ExecutionOutcome::Succeeded { .. }
        )
    }
}

/// Print the plan preview, including installed tools, missing tools, OS package names,
/// platform-specific binary notes, and the exact command requiring approval.
pub fn print_plan(plan: &InstallPlan, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "Platform: {}", plan.platform.display_name())?;

    if !plan.already_installed.is_empty() {
        writeln!(out, "\nAlready installed:")?;
        for item in &plan.already_installed {
            writeln!(
                out,
                "  ✓ {} ({}) — {}",
                item.entry.display_name, item.executable, item.version
            )?;
        }
    }

    if !plan.missing.is_empty() {
        writeln!(out, "\nMissing (will install):")?;
        for entry in &plan.missing {
            let info = entry
                .package_info_for(plan.platform.os_slug())
                .expect("supported platform must have package info");
            writeln!(
                out,
                "  • {} -> package '{}' (executable: '{}')",
                entry.display_name, info.package_name, info.executable_name
            )?;
            if let Some(note) = info.user_note {
                writeln!(out, "    {note}")?;
            }
        }
    }

    if let Some(cmd) = &plan.command {
        writeln!(out, "\nCommand to run:")?;
        writeln!(out, "  {}", cmd.display_line())?;
    }

    Ok(())
}

/// Prompt for explicit approval before making any system changes.
/// Treats empty input, EOF, and anything other than `y` / `yes` as a stop.
pub fn prompt_approval(out: &mut impl Write, input: &mut impl BufRead) -> io::Result<bool> {
    write!(out, "\nProceed with installation? [y/N]: ")?;
    out.flush()?;

    let mut line = String::new();
    let bytes_read = input.read_line(&mut line)?;
    if bytes_read == 0 {
        // EOF
        writeln!(out)?;
        return Ok(false);
    }

    let answer = line.trim().to_ascii_lowercase();
    Ok(matches!(answer.as_str(), "y" | "yes"))
}

/// Run the exact command vector via `std::process::Command` with provider keys stripped from env.
/// Leaves stdio inherited so OS password prompts (e.g. `sudo`) work natively.
pub fn run_install_command(cmd: &InstallCommand) -> io::Result<ExitStatus> {
    let mut child = Command::new(&cmd.program);
    child.args(&cmd.args);
    for key_var in auth::all_provider_env_vars() {
        child.env_remove(key_var);
    }
    child.status()
}

/// Verify all requested tools after execution.
pub fn verify_requested_tools(
    plan: &InstallPlan,
) -> (Vec<InstalledTool>, Vec<&'static CatalogEntry>) {
    let mut verified = Vec::new();
    let mut failed = Vec::new();

    for entry in &plan.requested {
        let info = entry
            .package_info_for(plan.platform.os_slug())
            .expect("supported platform must have package info");
        if let Some(version) =
            planner::check_tool_installed(info.executable_name, info.version_args)
        {
            verified.push(InstalledTool {
                entry,
                executable: info.executable_name,
                version,
            });
        } else {
            failed.push(*entry);
        }
    }

    (verified, failed)
}

/// Execute the complete preview -> approve -> run -> verify workflow.
pub fn execute_plan(
    plan: &InstallPlan,
    dry_run: bool,
    out: &mut impl Write,
    err: &mut impl Write,
    input: &mut impl BufRead,
) -> io::Result<ExecutionOutcome> {
    if plan.missing.is_empty() {
        writeln!(out, "Platform: {}", plan.platform.display_name())?;
        writeln!(out, "All requested tools are already installed:")?;
        for item in &plan.already_installed {
            writeln!(
                out,
                "  ✓ {} ({}) — {}",
                item.entry.display_name, item.executable, item.version
            )?;
            let info = item
                .entry
                .package_info_for(plan.platform.os_slug())
                .expect("supported platform must have package info");
            if let Some(note) = info.user_note {
                writeln!(out, "    {note}")?;
            }
        }
        writeln!(out, "\nNothing to install.")?;
        return Ok(ExecutionOutcome::NothingToInstall);
    }

    print_plan(plan, out)?;

    if dry_run {
        writeln!(out, "\nDry run complete. No changes were made.")?;
        return Ok(ExecutionOutcome::DryRun);
    }

    let approved = prompt_approval(out, input)?;
    if !approved {
        writeln!(out, "Installation cancelled. No changes were made.")?;
        return Ok(ExecutionOutcome::Cancelled);
    }

    let cmd = plan
        .command
        .as_ref()
        .expect("plan with missing tools must have an install command");

    writeln!(out, "\nRunning: {}", cmd.display_line())?;
    let status = match run_install_command(cmd) {
        Ok(status) => status,
        Err(e) => {
            writeln!(err, "Failed to launch '{}': {e}", cmd.program)?;
            return Ok(ExecutionOutcome::PartialOrFailed {
                verified: plan.already_installed.clone(),
                failed: plan.missing.clone(),
                command_exit_code: None,
            });
        }
    };

    let (verified, failed) = verify_requested_tools(plan);

    writeln!(out, "\nVerification:")?;
    for item in &verified {
        writeln!(
            out,
            "  ✓ {} ({}) — {}",
            item.entry.display_name, item.executable, item.version
        )?;
        let info = item
            .entry
            .package_info_for(plan.platform.os_slug())
            .expect("supported platform must have package info");
        if let Some(note) = info.user_note {
            writeln!(out, "    {note}")?;
        }
    }
    for entry in &failed {
        let info = entry
            .package_info_for(plan.platform.os_slug())
            .expect("supported platform must have package info");
        writeln!(
            err,
            "  ✗ {} (expected executable '{}' was not found or failed version check)",
            entry.display_name, info.executable_name
        )?;
    }

    if status.success() && failed.is_empty() {
        writeln!(out, "\nAll requested tools are installed and verified.")?;
        Ok(ExecutionOutcome::Succeeded { verified })
    } else {
        writeln!(
            err,
            "\nInstallation did not complete cleanly (exit code: {:?}). Re-run the command to recheck machine state and install any remaining tools.",
            status.code()
        )?;
        Ok(ExecutionOutcome::PartialOrFailed {
            verified,
            failed,
            command_exit_code: status.code(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn prompt_approval_accepts_only_explicit_yes() {
        for accepted in ["y\n", "Y\n", "yes\n", "YES\n", "  yes  \n"] {
            let mut out = Vec::new();
            let mut input = Cursor::new(accepted);
            assert!(prompt_approval(&mut out, &mut input).unwrap());
        }

        for rejected in ["", "\n", "n\n", "no\n", "maybe\n", "yep\n"] {
            let mut out = Vec::new();
            let mut input = Cursor::new(rejected);
            assert!(!prompt_approval(&mut out, &mut input).unwrap());
        }
    }
}
