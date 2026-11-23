//! Catalog of supported tools, package mappings, aliases, and executable checks.
//!
//! Keeps package names, accepted aliases, and executable checks together per platform.

/// Monotonic revision identifier for the built-in tool catalog.
/// Bump this whenever catalog entries, package mappings, or executable checks change.
pub const CATALOG_REVISION: &str = "v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPackageInfo {
    /// Package name passed to the package manager (e.g. "ripgrep", "fd", "fd-find")
    pub package_name: &'static str,
    /// Executable binary name checked on $PATH (e.g. "rg", "fd", "fdfind")
    pub executable_name: &'static str,
    /// Arguments used to check version after installation
    pub version_args: &'static [&'static str],
    /// Platform-specific user note (e.g. Ubuntu fdfind binary naming)
    pub user_note: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Canonical tool identifier (e.g. "ripgrep", "fd")
    pub id: &'static str,
    /// Human-friendly display name
    pub display_name: &'static str,
    /// Accepted aliases for explicit resolution
    pub aliases: &'static [&'static str],
    /// Descriptive summary for user prompts and LLM catalog injection
    pub description: &'static str,
    /// macOS Homebrew packaging information
    pub macos: PlatformPackageInfo,
    /// Ubuntu apt-get packaging information
    pub ubuntu: PlatformPackageInfo,
}

impl CatalogEntry {
    /// Retrieve packaging information for the specified target OS.
    pub fn package_info_for(&self, os: &str) -> Option<&PlatformPackageInfo> {
        match os {
            "macos" | "darwin" => Some(&self.macos),
            "ubuntu" | "debian" => Some(&self.ubuntu),
            _ => None,
        }
    }
}

pub static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "ripgrep",
        display_name: "ripgrep",
        aliases: &["ripgrep", "rg"],
        description: "Fast, regex-aware line-oriented search tool",
        macos: PlatformPackageInfo {
            package_name: "ripgrep",
            executable_name: "rg",
            version_args: &["--version"],
            user_note: None,
        },
        ubuntu: PlatformPackageInfo {
            package_name: "ripgrep",
            executable_name: "rg",
            version_args: &["--version"],
            user_note: None,
        },
    },
    CatalogEntry {
        id: "fd",
        display_name: "fd",
        aliases: &["fd", "fdfind", "fd-find"],
        description: "Simple, fast and user-friendly alternative to find",
        macos: PlatformPackageInfo {
            package_name: "fd",
            executable_name: "fd",
            version_args: &["--version"],
            user_note: None,
        },
        ubuntu: PlatformPackageInfo {
            package_name: "fd-find",
            executable_name: "fdfind",
            version_args: &["--version"],
            user_note: Some(
                "Note: On Ubuntu, the binary is installed as 'fdfind' (due to a Debian binary name collision with fdclone). You can run it with `fdfind` or create an alias (`alias fd=fdfind`).",
            ),
        },
    },
];

/// Find a catalog entry by canonical ID.
pub fn find_by_id(id: &str) -> Option<&'static CatalogEntry> {
    let lower = id.trim().to_ascii_lowercase();
    CATALOG.iter().find(|entry| entry.id == lower)
}

/// Find a catalog entry by alias or canonical ID.
pub fn find_by_alias(name: &str) -> Option<&'static CatalogEntry> {
    let lower = name.trim().to_ascii_lowercase();
    CATALOG
        .iter()
        .find(|entry| entry.id == lower || entry.aliases.iter().any(|alias| *alias == lower))
}

/// Return all supported catalog entries.
pub fn all_entries() -> &'static [CatalogEntry] {
    CATALOG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_ripgrep_by_aliases() {
        assert_eq!(find_by_alias("ripgrep").map(|e| e.id), Some("ripgrep"));
        assert_eq!(find_by_alias("rg").map(|e| e.id), Some("ripgrep"));
        assert_eq!(find_by_alias("RIPGREP").map(|e| e.id), Some("ripgrep"));
    }

    #[test]
    fn lookup_fd_by_aliases() {
        assert_eq!(find_by_alias("fd").map(|e| e.id), Some("fd"));
        assert_eq!(find_by_alias("fdfind").map(|e| e.id), Some("fd"));
        assert_eq!(find_by_alias("fd-find").map(|e| e.id), Some("fd"));
    }

    #[test]
    fn platform_package_info_distinguishes_ubuntu_and_macos() {
        let fd = find_by_id("fd").unwrap();

        let mac = fd.package_info_for("macos").unwrap();
        assert_eq!(mac.package_name, "fd");
        assert_eq!(mac.executable_name, "fd");
        assert!(mac.user_note.is_none());

        let ubuntu = fd.package_info_for("ubuntu").unwrap();
        assert_eq!(ubuntu.package_name, "fd-find");
        assert_eq!(ubuntu.executable_name, "fdfind");
        assert!(ubuntu.user_note.is_some());
    }
}
