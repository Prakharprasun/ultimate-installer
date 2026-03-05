//! Integration tests for the Ultimate Installer.
//!
//! These tests exercise real system interactions:
//! - Context Probe: runs against the actual machine
//! - Cold start detection: checks wizard triggers with no keys

use ultimate_installer::auth;
use ultimate_installer::context;

#[test]
fn context_probe_returns_valid_state() {
    // Run the full probe against the real machine.
    // This should never panic, regardless of what's installed.
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

    // The fingerprint must be non-empty and contain the intent
    let fp = state.fingerprint();
    assert!(fp.contains("integration_test"));
}

#[test]
fn cold_start_detection_works() {
    // has_any_api_key() should return a bool without panicking.
    // We can't control env vars in integration tests easily,
    // but we can at least verify the function runs.
    let _has_keys = auth::has_any_api_key();
}

#[test]
fn hash_of_probed_state_is_deterministic() {
    let state = context::probe("hash_test".into());
    let h1 = ultimate_installer::hash::hash_state(&state);
    let h2 = ultimate_installer::hash::hash_state(&state);
    assert_eq!(h1, h2, "hash of probed state must be deterministic");
}
