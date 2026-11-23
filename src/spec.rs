//! Canonical, versioned `InstallSpec` compiled before deterministic SHA-256 hashing.
//!
//! Inspired by NeuroSmith: user inputs and aliases are compiled into a fixed-field,
//! deterministically ordered specification before serializing to compact UTF-8 JSON
//! and hashing with SHA-256. Kept separate from the SeaHash machine fingerprint in `src/hash.rs`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{self, CatalogEntry};
use crate::planner::TargetPlatform;

/// Current schema version for `InstallSpec`.
pub const INSTALL_SPEC_SCHEMA_VERSION: u32 = 1;

/// Supported canonical actions in an `InstallSpec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecAction {
    Install,
}

/// Canonical install specification with fixed field order and no unordered maps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSpec {
    pub schema_version: u32,
    pub catalog_revision: String,
    pub action: SpecAction,
    pub target_os: String,
    pub cpu_arch: String,
    pub package_manager: String,
    pub tool_ids: Vec<String>,
}

/// Canonical serialized bytes paired with their lowercase hex SHA-256 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledSpec {
    pub spec: InstallSpec,
    pub canonical_json: Vec<u8>,
    pub sha256_hex: String,
}

impl InstallSpec {
    /// Compile a canonical `InstallSpec` from resolved catalog entries and platform metadata.
    ///
    /// Canonicalizes tool IDs by sorting lexicographically and deduplicating.
    pub fn from_entries(
        catalog_revision: impl Into<String>,
        platform: TargetPlatform,
        cpu_arch: impl Into<String>,
        entries: &[&'static CatalogEntry],
    ) -> Self {
        let mut tool_ids: Vec<String> = entries.iter().map(|e| e.id.to_string()).collect();
        tool_ids.sort();
        tool_ids.dedup();

        Self {
            schema_version: INSTALL_SPEC_SCHEMA_VERSION,
            catalog_revision: catalog_revision.into(),
            action: SpecAction::Install,
            target_os: platform.os_slug().to_string(),
            cpu_arch: cpu_arch.into(),
            package_manager: platform.package_manager_binary().to_string(),
            tool_ids,
        }
    }

    /// Resolve raw tool names/aliases through the catalog and compile a canonical `InstallSpec`.
    /// Returns `None` if any alias is not in the catalog or if `raw_names` is empty.
    pub fn from_aliases(
        catalog_revision: impl Into<String>,
        platform: TargetPlatform,
        cpu_arch: impl Into<String>,
        raw_names: &[&str],
    ) -> Option<Self> {
        if raw_names.is_empty() {
            return None;
        }
        let mut entries = Vec::with_capacity(raw_names.len());
        for raw in raw_names {
            entries.push(catalog::find_by_alias(raw)?);
        }
        Some(Self::from_entries(
            catalog_revision,
            platform,
            cpu_arch,
            &entries,
        ))
    }

    /// Serialize to compact UTF-8 JSON with fixed field order and compute its SHA-256 hex digest.
    pub fn compile(&self) -> CompiledSpec {
        let canonical_json =
            serde_json::to_vec(self).expect("InstallSpec serialization is infallible");
        let sha256_hex = sha256_hex(&canonical_json);
        CompiledSpec {
            spec: self.clone(),
            canonical_json,
            sha256_hex,
        }
    }
}

/// Compute lowercase hexadecimal SHA-256 digest of exact bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_canonical_json_and_sha256_digest() {
        let spec = InstallSpec::from_aliases(
            catalog::CATALOG_REVISION,
            TargetPlatform::Ubuntu,
            "x86_64",
            &["ripgrep", "fd"],
        )
        .unwrap();

        let compiled = spec.compile();
        let json_str = std::str::from_utf8(&compiled.canonical_json).unwrap();

        assert_eq!(
            json_str,
            r#"{"schema_version":1,"catalog_revision":"v1","action":"install","target_os":"ubuntu","cpu_arch":"x86_64","package_manager":"apt-get","tool_ids":["fd","ripgrep"]}"#
        );
        // Frozen SHA-256 of the exact compact UTF-8 JSON bytes above.
        assert_eq!(
            compiled.sha256_hex,
            "d82b4b1d7e511bac4a930f5aefc4ca4b948aff7146fd514e5311c3074186f8a6"
        );
    }

    #[test]
    fn aliases_and_reordered_tools_produce_identical_digest() {
        let a =
            InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["ripgrep", "fd"])
                .unwrap()
                .compile();
        let b = InstallSpec::from_aliases(
            "v1",
            TargetPlatform::MacOs,
            "aarch64",
            &["fd-find", "rg", "fdfind"],
        )
        .unwrap()
        .compile();

        assert_eq!(a.canonical_json, b.canonical_json);
        assert_eq!(a.sha256_hex, b.sha256_hex);
    }

    #[test]
    fn changing_platform_arch_or_catalog_revision_changes_digest() {
        let base = InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["fd", "rg"])
            .unwrap()
            .compile();
        let diff_os =
            InstallSpec::from_aliases("v1", TargetPlatform::Ubuntu, "aarch64", &["fd", "rg"])
                .unwrap()
                .compile();
        let diff_arch =
            InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "x86_64", &["fd", "rg"])
                .unwrap()
                .compile();
        let diff_rev =
            InstallSpec::from_aliases("v2", TargetPlatform::MacOs, "aarch64", &["fd", "rg"])
                .unwrap()
                .compile();

        assert_ne!(base.sha256_hex, diff_os.sha256_hex);
        assert_ne!(base.sha256_hex, diff_arch.sha256_hex);
        assert_ne!(base.sha256_hex, diff_rev.sha256_hex);
    }
}
