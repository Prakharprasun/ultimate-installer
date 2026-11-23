//! Installation planner — validates platform prerequisites, checks existing tools,
//! and produces a deterministic command vector for preview and execution.

use std::fmt;
use std::process::Command;

use crate::auth;
use crate::catalog::CatalogEntry;
use crate::context;

/// Supported installation target platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    MacOs,
    Ubuntu,
}

impl TargetPlatform {
    pub fn os_slug(&self) -> &'static str {
        match self {
            TargetPlatform::MacOs => "macos",
            TargetPlatform::Ubuntu => "ubuntu",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            TargetPlatform::MacOs => "macOS (Homebrew)",
            TargetPlatform::Ubuntu => "Ubuntu (apt-get)",
        }
    }

    pub fn package_manager_binary(&self) -> &'static str {
        match self {
            TargetPlatform::MacOs => "brew",
            TargetPlatform::Ubuntu => "apt-get",
        }
    }
}

/// Errors encountered while validating the platform or building an install plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    UnsupportedOs { os: String, message: String },
    MissingPrerequisite { message: String },
    NoToolsRequested,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::UnsupportedOs { message, .. } => write!(f, "{message}"),
            PlanError::MissingPrerequisite { message } => write!(f, "{message}"),
            PlanError::NoToolsRequested => write!(f, "No tools were requested to install."),
        }
    }
}

impl std::error::Error for PlanError {}

/// A tool that is already installed and verified working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTool {
    pub entry: &'static CatalogEntry,
    pub executable: &'static str,
    pub version: String,
}

/// Exact executable and argument vector used for both preview and execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl InstallCommand {
    /// Format the command for human-readable preview.
    pub fn display_line(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 1);
        parts.push(self.program.clone());
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

/// A validated installation plan for a set of catalog tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub platform: TargetPlatform,
    pub requested: Vec<&'static CatalogEntry>,
    pub already_installed: Vec<InstalledTool>,
    pub missing: Vec<&'static CatalogEntry>,
    pub command: Option<InstallCommand>,
}

/// Check whether the current process is running as root (UID 0).
pub fn is_effective_root() -> bool {
    #[cfg(unix)]
    {
        if std::env::var("ULTI_ASSUME_NON_ROOT").ok().as_deref() == Some("1") {
            return false;
        }
        Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim() == "0")
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Check whether a tool executable is on `$PATH` and responds successfully to its version flag.
/// Strips provider API keys from the child environment.
pub fn check_tool_installed(executable: &str, version_args: &[&str]) -> Option<String> {
    which::which(executable).ok()?;
    let mut cmd = Command::new(executable);
    cmd.args(version_args);
    for key_var in auth::all_provider_env_vars() {
        cmd.env_remove(key_var);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            stderr
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| format!("{executable} (installed)"));
    Some(first_line)
}

/// Validate the target OS and required package-manager executable.
pub fn resolve_platform<F>(os: &str, has_bin: F) -> Result<TargetPlatform, PlanError>
where
    F: Fn(&str) -> bool,
{
    match os {
        "macos" | "darwin" => {
            if !has_bin("brew") {
                return Err(PlanError::MissingPrerequisite {
                    message: "Homebrew ('brew') is required on macOS but was not found on PATH.\nInstall Homebrew from https://brew.sh and re-run your command.".to_string(),
                });
            }
            Ok(TargetPlatform::MacOs)
        }
        "ubuntu" => {
            if !has_bin("apt-get") {
                return Err(PlanError::MissingPrerequisite {
                    message: "'apt-get' is required on Ubuntu but was not found on PATH.\nEnsure apt is installed and available on PATH.".to_string(),
                });
            }
            Ok(TargetPlatform::Ubuntu)
        }
        "windows" => Err(PlanError::UnsupportedOs {
            os: os.to_string(),
            message: "Windows is not currently supported for automated installs.\nSupported platforms are macOS (with Homebrew) and Ubuntu (with apt-get).".to_string(),
        }),
        other => Err(PlanError::UnsupportedOs {
            os: other.to_string(),
            message: format!(
                "Unsupported platform '{other}'.\nSupported platforms are macOS (with Homebrew) and Ubuntu (with apt-get)."
            ),
        }),
    }
}

/// Build an install plan against the live local machine.
pub fn build_plan(tools: Vec<&'static CatalogEntry>) -> Result<InstallPlan, PlanError> {
    let os = context::detect_os();
    let is_root = is_effective_root();
    build_plan_with_probes(
        tools,
        &os,
        context::has_executable,
        check_tool_installed,
        is_root,
    )
}

/// Build an install plan with injectable probes for deterministic unit testing.
pub fn build_plan_with_probes<B, T>(
    tools: Vec<&'static CatalogEntry>,
    os: &str,
    has_bin: B,
    check_tool: T,
    is_root: bool,
) -> Result<InstallPlan, PlanError>
where
    B: Fn(&str) -> bool,
    T: Fn(&str, &[&str]) -> Option<String>,
{
    if tools.is_empty() {
        return Err(PlanError::NoToolsRequested);
    }

    let platform = resolve_platform(os, &has_bin)?;

    let mut already_installed = Vec::new();
    let mut missing = Vec::new();

    for entry in &tools {
        let info = entry
            .package_info_for(platform.os_slug())
            .expect("supported platform must have package info");

        if let Some(version) = check_tool(info.executable_name, info.version_args) {
            already_installed.push(InstalledTool {
                entry,
                executable: info.executable_name,
                version,
            });
        } else {
            missing.push(*entry);
        }
    }

    let command = if missing.is_empty() {
        None
    } else {
        let packages: Vec<String> = missing
            .iter()
            .map(|entry| {
                entry
                    .package_info_for(platform.os_slug())
                    .expect("supported platform must have package info")
                    .package_name
                    .to_string()
            })
            .collect();

        match platform {
            TargetPlatform::MacOs => {
                let mut args = vec!["install".to_string()];
                args.extend(packages);
                Some(InstallCommand {
                    program: "brew".to_string(),
                    args,
                })
            }
            TargetPlatform::Ubuntu => {
                if is_root {
                    let mut args = vec!["install".to_string(), "-y".to_string()];
                    args.extend(packages);
                    Some(InstallCommand {
                        program: "apt-get".to_string(),
                        args,
                    })
                } else {
                    if !has_bin("sudo") {
                        return Err(PlanError::MissingPrerequisite {
                            message: "'sudo' is required to run apt-get as a non-root user on Ubuntu, but 'sudo' was not found on PATH.\nInstall sudo or run as root.".to_string(),
                        });
                    }
                    let mut args = vec![
                        "apt-get".to_string(),
                        "install".to_string(),
                        "-y".to_string(),
                    ];
                    args.extend(packages);
                    Some(InstallCommand {
                        program: "sudo".to_string(),
                        args,
                    })
                }
            }
        }
    };

    Ok(InstallPlan {
        platform,
        requested: tools,
        already_installed,
        missing,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    #[test]
    fn macos_plan_installs_only_missing_tools() {
        let tools = vec![
            catalog::find_by_id("ripgrep").unwrap(),
            catalog::find_by_id("fd").unwrap(),
        ];

        let plan = build_plan_with_probes(
            tools,
            "macos",
            |bin| bin == "brew",
            |exe, _| {
                if exe == "rg" {
                    Some("ripgrep 14.1.0".to_string())
                } else {
                    None
                }
            },
            false,
        )
        .unwrap();

        assert_eq!(plan.platform, TargetPlatform::MacOs);
        assert_eq!(plan.already_installed.len(), 1);
        assert_eq!(plan.already_installed[0].entry.id, "ripgrep");
        assert_eq!(plan.missing.len(), 1);
        assert_eq!(plan.missing[0].id, "fd");
        assert_eq!(
            plan.command,
            Some(InstallCommand {
                program: "brew".to_string(),
                args: vec!["install".to_string(), "fd".to_string()],
            })
        );
    }

    #[test]
    fn ubuntu_non_root_plan_uses_sudo_and_fd_find() {
        let tools = vec![
            catalog::find_by_id("ripgrep").unwrap(),
            catalog::find_by_id("fd").unwrap(),
        ];

        let plan = build_plan_with_probes(
            tools,
            "ubuntu",
            |bin| bin == "apt-get" || bin == "sudo",
            |_, _| None,
            false,
        )
        .unwrap();

        assert_eq!(plan.platform, TargetPlatform::Ubuntu);
        assert_eq!(
            plan.command,
            Some(InstallCommand {
                program: "sudo".to_string(),
                args: vec![
                    "apt-get".to_string(),
                    "install".to_string(),
                    "-y".to_string(),
                    "ripgrep".to_string(),
                    "fd-find".to_string(),
                ],
            })
        );
    }

    #[test]
    fn all_present_produces_no_command() {
        let tools = vec![
            catalog::find_by_id("ripgrep").unwrap(),
            catalog::find_by_id("fd").unwrap(),
        ];

        let plan = build_plan_with_probes(
            tools,
            "macos",
            |bin| bin == "brew",
            |exe, _| Some(format!("{exe} 1.0.0")),
            false,
        )
        .unwrap();

        assert!(plan.missing.is_empty());
        assert!(plan.command.is_none());
    }

    #[test]
    fn missing_brew_returns_actionable_error() {
        let tools = vec![catalog::find_by_id("ripgrep").unwrap()];
        let err =
            build_plan_with_probes(tools, "macos", |_| false, |_, _| None, false).unwrap_err();
        assert!(err.to_string().contains("Homebrew"));
        assert!(err.to_string().contains("https://brew.sh"));
    }

    #[test]
    fn windows_explains_supported_platforms() {
        let tools = vec![catalog::find_by_id("ripgrep").unwrap()];
        let err =
            build_plan_with_probes(tools, "windows", |_| true, |_, _| None, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Windows"));
        assert!(msg.contains("macOS"));
        assert!(msg.contains("Ubuntu"));
    }
}
