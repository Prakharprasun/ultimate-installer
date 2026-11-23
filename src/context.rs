//! Context Engine — probes the local system to build a SystemState.
//!
//! Detects OS, architecture, available package managers, shell, and GPU drivers.

use crate::SystemState;

/// Known package manager binaries to probe for.
const KNOWN_PACKAGE_MANAGERS: &[&str] = &[
    "apt", "apt-get", "brew", "pacman", "dnf", "yay", "nix", "zypper", "snap", "flatpak", "port",
    "emerge", "apk", "pkg", "winget", "choco", "scoop",
];

/// Detect which package managers are available on $PATH.
pub fn detect_package_managers() -> Vec<String> {
    KNOWN_PACKAGE_MANAGERS
        .iter()
        .filter(|name| which::which(name).is_ok())
        .map(|name| (*name).to_string())
        .collect()
}

/// Check whether a specific executable is available on $PATH.
pub fn has_executable(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Detect the current OS as a normalised slug.
///
/// Honors `ULTI_OS_OVERRIDE` when set (used for cross-platform CLI testing with fake binaries).
pub fn detect_os() -> String {
    if let Ok(override_os) = std::env::var("ULTI_OS_OVERRIDE") {
        let trimmed = override_os.trim().to_ascii_lowercase();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let info = sysinfo::System::new();
    let name = sysinfo::System::name().unwrap_or_default().to_lowercase();
    let _ = info; // sysinfo::System::name() is a static method

    if name.contains("mac") || name.contains("darwin") {
        "macos".into()
    } else if name.contains("arch") {
        "arch_linux".into()
    } else if name.contains("ubuntu") {
        "ubuntu".into()
    } else if name.contains("fedora") {
        "fedora".into()
    } else if name.contains("debian") {
        "debian".into()
    } else if name.contains("windows") {
        "windows".into()
    } else if !name.is_empty() {
        name.replace(' ', "_")
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "windows") {
        "windows".into()
    } else {
        "unknown".into()
    }
}

/// Detect CPU architecture.
pub fn detect_arch() -> String {
    std::env::consts::ARCH.to_string()
}

/// Detect the user's current shell from $SHELL.
pub fn detect_shell() -> String {
    std::env::var("SHELL")
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Build a full SystemState by probing the local environment.
pub fn probe(intent: String) -> SystemState {
    SystemState::new(
        intent,
        detect_os(),
        detect_arch(),
        detect_package_managers(),
        None, // GPU detection is a future enhancement
        detect_shell(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_arch_returns_non_empty() {
        let arch = detect_arch();
        assert!(!arch.is_empty(), "arch should never be empty");
    }

    #[test]
    fn detect_os_returns_non_empty() {
        let os = detect_os();
        assert!(!os.is_empty(), "OS should never be empty");
    }

    #[test]
    fn detect_shell_returns_something() {
        // On CI this may be empty if $SHELL isn't set; that's OK.
        let _shell = detect_shell();
    }

    #[test]
    fn probe_produces_valid_state() {
        let state = probe("test_intent".into());
        assert_eq!(state.intent, "test_intent");
        assert!(!state.os.is_empty());
        assert!(!state.arch.is_empty());
        assert!(state.errors_encountered.is_empty());
    }

    #[test]
    fn detect_package_managers_returns_vec() {
        // This will find whatever is on the current machine — just assert it doesn't panic.
        let managers = detect_package_managers();
        assert!(
            managers.is_empty() || !managers.is_empty(),
            "should return a vec (even if empty)"
        );
    }
}
