//! Persistent local SQLite cache for verified installs and descriptive request resolutions.
//!
//! Stores canonical `InstallSpec` bytes alongside SHA-256 digests, verified command records
//! keyed by exact program + ordered argv, and exact trimmed descriptive request mappings.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::catalog::{self, CatalogEntry};
use crate::planner::{InstallCommand, InstalledTool};
use crate::spec::CompiledSpec;

/// Current SQLite cache schema version.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Resolver namespace for OpenAI descriptive requests.
pub const RESOLVER_OPENAI: &str = "openai";

/// Environment variable allowing tests or users to redirect the cache directory.
pub const CACHE_DIR_ENV: &str = "ULTI_CACHE_DIR";

/// SQLite database filename inside the application data directory.
pub const CACHE_DB_FILENAME: &str = "cache.db";

/// Recorded version observation for a verified tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedToolRecord {
    pub tool_id: String,
    pub executable: String,
    pub version: String,
}

/// A previously verified command record for a specific `InstallSpec` and exact argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCommandRecord {
    pub spec_hash: String,
    pub program: String,
    pub args: Vec<String>,
    pub verified_tools: Vec<VerifiedToolRecord>,
    pub last_verified_at: i64,
    pub success_count: i64,
}

/// Optional descriptive request resolution to persist alongside a verified install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResolutionWrite<'a> {
    pub resolver: &'a str,
    pub catalog_revision: &'a str,
    pub trimmed_request: &'a str,
    pub tool_ids: Vec<String>,
}

/// Errors returned by the local SQLite cache.
#[derive(Debug)]
pub enum CacheError {
    NoDataDir,
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    SpecCollision { spec_hash: String },
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::NoDataDir => {
                write!(f, "could not determine user application data directory")
            }
            CacheError::Io(e) => write!(f, "cache I/O error: {e}"),
            CacheError::Sqlite(e) => write!(f, "SQLite cache error: {e}"),
            CacheError::Json(e) => write!(f, "cache JSON error: {e}"),
            CacheError::SpecCollision { spec_hash } => {
                write!(f, "canonical spec byte mismatch for digest {spec_hash}")
            }
        }
    }
}

impl std::error::Error for CacheError {}

impl From<std::io::Error> for CacheError {
    fn from(e: std::io::Error) -> Self {
        CacheError::Io(e)
    }
}

impl From<rusqlite::Error> for CacheError {
    fn from(e: rusqlite::Error) -> Self {
        CacheError::Sqlite(e)
    }
}

impl From<serde_json::Error> for CacheError {
    fn from(e: serde_json::Error) -> Self {
        CacheError::Json(e)
    }
}

/// Resolve the default per-user application data directory for the cache DB.
///
/// Honors `ULTI_CACHE_DIR` when set (used by tests to point at a temporary directory).
pub fn default_cache_dir() -> Result<PathBuf, CacheError> {
    if let Ok(override_dir) = std::env::var(CACHE_DIR_ENV) {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or(CacheError::NoDataDir)?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("ultimate-installer"))
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .ok_or(CacheError::NoDataDir)?;
        Ok(PathBuf::from(base).join("ultimate-installer"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
            && !xdg.is_empty()
        {
            return Ok(PathBuf::from(xdg).join("ultimate-installer"));
        }
        let home = std::env::var_os("HOME").ok_or(CacheError::NoDataDir)?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("ultimate-installer"))
    }
}

/// Handle to the persistent local SQLite cache.
pub struct LocalCache {
    path: PathBuf,
    conn: Connection,
}

impl LocalCache {
    /// Open (and initialize if needed) the default per-user cache database.
    pub fn open_default() -> Result<Self, CacheError> {
        let dir = default_cache_dir()?;
        Self::open_in_dir(&dir)
    }

    /// Open (and initialize if needed) a cache database inside the specified directory.
    pub fn open_in_dir(dir: &Path) -> Result<Self, CacheError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(CACHE_DB_FILENAME);
        Self::open_path(&path)
    }

    /// Open (and initialize if needed) a cache database at a specific file path.
    pub fn open_path(path: &Path) -> Result<Self, CacheError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        init_schema(&conn)?;
        Ok(Self {
            path: path.to_path_buf(),
            conn,
        })
    }

    /// Return the path to the SQLite database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Look up a previously verified exact command under the given compiled spec.
    ///
    /// Verifies that the stored canonical spec JSON bytes match `compiled.canonical_json`
    /// byte-for-byte before returning a hit.
    pub fn lookup_verified_command(
        &self,
        compiled: &CompiledSpec,
        cmd: &InstallCommand,
    ) -> Result<Option<VerifiedCommandRecord>, CacheError> {
        let stored_bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT canonical_json FROM install_specs WHERE spec_hash = ?1",
                params![compiled.sha256_hex],
                |row| row.get(0),
            )
            .optional()?;

        let Some(bytes) = stored_bytes else {
            return Ok(None);
        };
        if bytes != compiled.canonical_json {
            return Err(CacheError::SpecCollision {
                spec_hash: compiled.sha256_hex.clone(),
            });
        }

        let argv_json = encode_argv(cmd)?;
        let record = self
            .conn
            .query_row(
                "SELECT program, args_json, verified_tools_json, last_verified_at, success_count
                 FROM verified_commands
                 WHERE spec_hash = ?1 AND argv_json = ?2",
                params![compiled.sha256_hex, argv_json],
                |row| {
                    let program: String = row.get(0)?;
                    let args_json: String = row.get(1)?;
                    let verified_tools_json: String = row.get(2)?;
                    let last_verified_at: i64 = row.get(3)?;
                    let success_count: i64 = row.get(4)?;
                    Ok((
                        program,
                        args_json,
                        verified_tools_json,
                        last_verified_at,
                        success_count,
                    ))
                },
            )
            .optional()?;

        let Some((program, args_json, verified_tools_json, last_verified_at, success_count)) =
            record
        else {
            return Ok(None);
        };

        let args: Vec<String> = serde_json::from_str(&args_json)?;
        let verified_tools: Vec<VerifiedToolRecord> = serde_json::from_str(&verified_tools_json)?;

        Ok(Some(VerifiedCommandRecord {
            spec_hash: compiled.sha256_hex.clone(),
            program,
            args,
            verified_tools,
            last_verified_at,
            success_count,
        }))
    }

    /// Look up previously validated canonical tool IDs for an exact trimmed descriptive request,
    /// scoped by resolver and catalog revision.
    ///
    /// Revalidates every retrieved ID against the current catalog; returns `Ok(None)` if
    /// missing, empty, or containing any ID no longer valid in the current catalog.
    pub fn lookup_request_resolution(
        &self,
        resolver: &str,
        catalog_revision: &str,
        trimmed_request: &str,
    ) -> Result<Option<Vec<&'static CatalogEntry>>, CacheError> {
        let tool_ids_json: Option<String> = self
            .conn
            .query_row(
                "SELECT tool_ids_json
                 FROM request_resolutions
                 WHERE resolver = ?1 AND catalog_revision = ?2 AND trimmed_request = ?3",
                params![resolver, catalog_revision, trimmed_request],
                |row| row.get(0),
            )
            .optional()?;

        let Some(json) = tool_ids_json else {
            return Ok(None);
        };

        let raw_ids: Vec<String> = match serde_json::from_str(&json) {
            Ok(ids) => ids,
            Err(_) => return Ok(None),
        };
        if raw_ids.is_empty() {
            return Ok(None);
        }

        let mut entries = Vec::with_capacity(raw_ids.len());
        for id in &raw_ids {
            let Some(entry) = catalog::find_by_id(id) else {
                return Ok(None);
            };
            if entry.id != id {
                return Ok(None);
            }
            if !entries
                .iter()
                .any(|existing: &&CatalogEntry| existing.id == entry.id)
            {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|e| e.id);
        Ok(Some(entries))
    }

    /// Atomically record a verified install (spec + exact command + optional provider resolution)
    /// in a single `IMMEDIATE` transaction.
    ///
    /// Must only be called after `ExecutionOutcome::Succeeded { verified }`.
    pub fn record_verified_install(
        &mut self,
        compiled: &CompiledSpec,
        cmd: &InstallCommand,
        verified: &[InstalledTool],
        provider_resolution: Option<&ProviderResolutionWrite<'_>>,
    ) -> Result<(), CacheError> {
        let now = unix_timestamp_secs();
        let tool_ids_json = serde_json::to_string(&compiled.spec.tool_ids)?;
        let argv_json = encode_argv(cmd)?;
        let args_json = serde_json::to_string(&cmd.args)?;

        let mut verified_records: Vec<VerifiedToolRecord> = verified
            .iter()
            .map(|t| VerifiedToolRecord {
                tool_id: t.entry.id.to_string(),
                executable: t.executable.to_string(),
                version: t.version.clone(),
            })
            .collect();
        verified_records.sort_by(|a, b| a.tool_id.cmp(&b.tool_id));
        let verified_tools_json = serde_json::to_string(&verified_records)?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Check existing spec bytes if present to detect any digest mismatch.
        let existing_bytes: Option<Vec<u8>> = tx
            .query_row(
                "SELECT canonical_json FROM install_specs WHERE spec_hash = ?1",
                params![compiled.sha256_hex],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(bytes) = existing_bytes {
            if bytes != compiled.canonical_json {
                return Err(CacheError::SpecCollision {
                    spec_hash: compiled.sha256_hex.clone(),
                });
            }
        } else {
            tx.execute(
                "INSERT INTO install_specs (
                    spec_hash, canonical_json, schema_version, catalog_revision,
                    action, target_os, cpu_arch, package_manager, tool_ids_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    compiled.sha256_hex,
                    compiled.canonical_json,
                    compiled.spec.schema_version,
                    compiled.spec.catalog_revision,
                    "install",
                    compiled.spec.target_os,
                    compiled.spec.cpu_arch,
                    compiled.spec.package_manager,
                    tool_ids_json,
                    now,
                ],
            )?;
        }

        tx.execute(
            "INSERT INTO verified_commands (
                spec_hash, argv_json, program, args_json,
                verified_tools_json, last_verified_at, success_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
             ON CONFLICT(spec_hash, argv_json) DO UPDATE SET
                verified_tools_json = excluded.verified_tools_json,
                last_verified_at = excluded.last_verified_at,
                success_count = verified_commands.success_count + 1",
            params![
                compiled.sha256_hex,
                argv_json,
                cmd.program,
                args_json,
                verified_tools_json,
                now,
            ],
        )?;

        if let Some(res) = provider_resolution {
            let mut canonical_ids = res.tool_ids.clone();
            canonical_ids.sort();
            canonical_ids.dedup();
            let res_ids_json = serde_json::to_string(&canonical_ids)?;

            tx.execute(
                "INSERT INTO request_resolutions (
                    resolver, catalog_revision, trimmed_request,
                    tool_ids_json, last_verified_at, success_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 1)
                 ON CONFLICT(resolver, catalog_revision, trimmed_request) DO UPDATE SET
                    tool_ids_json = excluded.tool_ids_json,
                    last_verified_at = excluded.last_verified_at,
                    success_count = request_resolutions.success_count + 1",
                params![
                    res.resolver,
                    res.catalog_revision,
                    res.trimmed_request,
                    res_ids_json,
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

fn init_schema(conn: &Connection) -> Result<(), CacheError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS install_specs (
            spec_hash TEXT PRIMARY KEY,
            canonical_json BLOB NOT NULL,
            schema_version INTEGER NOT NULL,
            catalog_revision TEXT NOT NULL,
            action TEXT NOT NULL,
            target_os TEXT NOT NULL,
            cpu_arch TEXT NOT NULL,
            package_manager TEXT NOT NULL,
            tool_ids_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS verified_commands (
            spec_hash TEXT NOT NULL,
            argv_json TEXT NOT NULL,
            program TEXT NOT NULL,
            args_json TEXT NOT NULL,
            verified_tools_json TEXT NOT NULL,
            last_verified_at INTEGER NOT NULL,
            success_count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (spec_hash, argv_json),
            FOREIGN KEY (spec_hash) REFERENCES install_specs(spec_hash) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS request_resolutions (
            resolver TEXT NOT NULL,
            catalog_revision TEXT NOT NULL,
            trimmed_request TEXT NOT NULL,
            tool_ids_json TEXT NOT NULL,
            last_verified_at INTEGER NOT NULL,
            success_count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (resolver, catalog_revision, trimmed_request)
        );",
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', ?1)",
        params![CACHE_SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

/// Encode `[program, ...args]` as a compact JSON string array so argument boundaries,
/// ordering, and privilege wrappers (`sudo`) are preserved unambiguously.
fn encode_argv(cmd: &InstallCommand) -> Result<String, serde_json::Error> {
    let mut full = Vec::with_capacity(cmd.args.len() + 1);
    full.push(cmd.program.as_str());
    for arg in &cmd.args {
        full.push(arg.as_str());
    }
    serde_json::to_string(&full)
}

fn unix_timestamp_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::TargetPlatform;
    use crate::spec::InstallSpec;
    use tempfile::TempDir;

    fn sample_verified() -> Vec<InstalledTool> {
        vec![
            InstalledTool {
                entry: catalog::find_by_id("fd").unwrap(),
                executable: "fd",
                version: "fd 10.2.0".to_string(),
            },
            InstalledTool {
                entry: catalog::find_by_id("ripgrep").unwrap(),
                executable: "rg",
                version: "ripgrep 14.1.0".to_string(),
            },
        ]
    }

    #[test]
    fn stores_spec_once_and_keeps_distinct_commands_separate() {
        let tmp = TempDir::new().unwrap();
        let mut cache = LocalCache::open_in_dir(tmp.path()).unwrap();

        let compiled =
            InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["ripgrep", "fd"])
                .unwrap()
                .compile();

        let full_cmd = InstallCommand {
            program: "brew".into(),
            args: vec!["install".into(), "fd".into(), "ripgrep".into()],
        };
        let partial_cmd = InstallCommand {
            program: "brew".into(),
            args: vec!["install".into(), "fd".into()],
        };

        cache
            .record_verified_install(&compiled, &full_cmd, &sample_verified(), None)
            .unwrap();
        cache
            .record_verified_install(&compiled, &partial_cmd, &sample_verified(), None)
            .unwrap();
        cache
            .record_verified_install(&compiled, &full_cmd, &sample_verified(), None)
            .unwrap();

        let full_rec = cache
            .lookup_verified_command(&compiled, &full_cmd)
            .unwrap()
            .unwrap();
        let partial_rec = cache
            .lookup_verified_command(&compiled, &partial_cmd)
            .unwrap()
            .unwrap();

        assert_eq!(full_rec.success_count, 2);
        assert_eq!(partial_rec.success_count, 1);

        // Spec stored exactly once
        let spec_count: i64 = cache
            .conn
            .query_row("SELECT COUNT(*) FROM install_specs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(spec_count, 1);
    }

    #[test]
    fn preserves_argument_boundaries_order_and_sudo() {
        let tmp = TempDir::new().unwrap();
        let mut cache = LocalCache::open_in_dir(tmp.path()).unwrap();
        let compiled =
            InstallSpec::from_aliases("v1", TargetPlatform::Ubuntu, "x86_64", &["fd", "ripgrep"])
                .unwrap()
                .compile();

        let with_sudo = InstallCommand {
            program: "sudo".into(),
            args: vec![
                "apt-get".into(),
                "install".into(),
                "-y".into(),
                "fd-find".into(),
                "ripgrep".into(),
            ],
        };
        let without_sudo = InstallCommand {
            program: "apt-get".into(),
            args: vec![
                "install".into(),
                "-y".into(),
                "fd-find".into(),
                "ripgrep".into(),
            ],
        };
        let reordered = InstallCommand {
            program: "sudo".into(),
            args: vec![
                "apt-get".into(),
                "install".into(),
                "-y".into(),
                "ripgrep".into(),
                "fd-find".into(),
            ],
        };
        let merged_arg = InstallCommand {
            program: "sudo".into(),
            args: vec!["apt-get install".into(), "-y".into(), "fd-find".into()],
        };

        cache
            .record_verified_install(&compiled, &with_sudo, &sample_verified(), None)
            .unwrap();

        assert!(
            cache
                .lookup_verified_command(&compiled, &with_sudo)
                .unwrap()
                .is_some()
        );
        assert!(
            cache
                .lookup_verified_command(&compiled, &without_sudo)
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .lookup_verified_command(&compiled, &reordered)
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .lookup_verified_command(&compiled, &merged_arg)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn detects_canonical_json_byte_mismatch_on_lookup() {
        let tmp = TempDir::new().unwrap();
        let mut cache = LocalCache::open_in_dir(tmp.path()).unwrap();
        let compiled = InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["fd"])
            .unwrap()
            .compile();
        let cmd = InstallCommand {
            program: "brew".into(),
            args: vec!["install".into(), "fd".into()],
        };
        cache
            .record_verified_install(&compiled, &cmd, &sample_verified(), None)
            .unwrap();

        // Tamper with stored canonical_json bytes
        cache
            .conn
            .execute(
                "UPDATE install_specs SET canonical_json = ?1 WHERE spec_hash = ?2",
                params![b"{}".as_slice(), compiled.sha256_hex],
            )
            .unwrap();

        let err = cache.lookup_verified_command(&compiled, &cmd).unwrap_err();
        assert!(matches!(err, CacheError::SpecCollision { .. }));
    }

    #[test]
    fn concurrent_writes_are_atomic_and_never_lose_updates() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        // Initialize schema once
        let _ = LocalCache::open_in_dir(&dir).unwrap();

        let compiled =
            InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["fd", "ripgrep"])
                .unwrap()
                .compile();
        let cmd = InstallCommand {
            program: "brew".into(),
            args: vec!["install".into(), "fd".into(), "ripgrep".into()],
        };

        let mut handles = Vec::new();
        for _ in 0..8 {
            let dir_clone = dir.clone();
            let compiled_clone = compiled.clone();
            let cmd_clone = cmd.clone();
            handles.push(std::thread::spawn(move || {
                let mut cache = LocalCache::open_in_dir(&dir_clone).unwrap();
                for _ in 0..5 {
                    cache
                        .record_verified_install(
                            &compiled_clone,
                            &cmd_clone,
                            &sample_verified(),
                            None,
                        )
                        .unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let cache = LocalCache::open_in_dir(&dir).unwrap();
        let rec = cache
            .lookup_verified_command(&compiled, &cmd)
            .unwrap()
            .unwrap();
        assert_eq!(rec.success_count, 40);
    }

    #[test]
    fn request_resolution_preserves_exact_text_and_revalidates_catalog() {
        let tmp = TempDir::new().unwrap();
        let mut cache = LocalCache::open_in_dir(tmp.path()).unwrap();
        let compiled =
            InstallSpec::from_aliases("v1", TargetPlatform::MacOs, "aarch64", &["fd", "ripgrep"])
                .unwrap()
                .compile();
        let cmd = InstallCommand {
            program: "brew".into(),
            args: vec!["install".into(), "fd".into(), "ripgrep".into()],
        };

        let res = ProviderResolutionWrite {
            resolver: RESOLVER_OPENAI,
            catalog_revision: catalog::CATALOG_REVISION,
            trimmed_request: "Set me up with ripgrep, and fd!",
            tool_ids: vec!["ripgrep".into(), "fd".into()],
        };
        cache
            .record_verified_install(&compiled, &cmd, &sample_verified(), Some(&res))
            .unwrap();

        // Exact match hits
        let hit = cache
            .lookup_request_resolution(
                RESOLVER_OPENAI,
                catalog::CATALOG_REVISION,
                "Set me up with ripgrep, and fd!",
            )
            .unwrap()
            .unwrap();
        let ids: Vec<&str> = hit.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["fd", "ripgrep"]);

        // Punctuation / negation / word order / revision differences miss
        assert!(
            cache
                .lookup_request_resolution(
                    RESOLVER_OPENAI,
                    catalog::CATALOG_REVISION,
                    "Set me up with ripgrep and fd"
                )
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .lookup_request_resolution(
                    RESOLVER_OPENAI,
                    catalog::CATALOG_REVISION,
                    "Do not set me up with ripgrep, and fd!"
                )
                .unwrap()
                .is_none()
        );
        assert!(
            cache
                .lookup_request_resolution(RESOLVER_OPENAI, "v2", "Set me up with ripgrep, and fd!")
                .unwrap()
                .is_none()
        );

        // Stale unknown ID in DB fails catalog revalidation
        cache
            .conn
            .execute(
                "UPDATE request_resolutions SET tool_ids_json = '[\"fd\",\"removed_tool\"]'",
                [],
            )
            .unwrap();
        assert!(
            cache
                .lookup_request_resolution(
                    RESOLVER_OPENAI,
                    catalog::CATALOG_REVISION,
                    "Set me up with ripgrep, and fd!"
                )
                .unwrap()
                .is_none()
        );
    }
}
