//! Hashing utilities for the state-aware Bloom filter.
//!
//! Uses SeaHash for fast, deterministic hashing of the SystemState fingerprint.

use seahash::hash as seahash;

use crate::SystemState;

/// Hash a SystemState into a u64 for Bloom filter lookups.
/// The hash is deterministic: same state always produces the same hash.
pub fn hash_state(state: &SystemState) -> u64 {
    let fingerprint = state.fingerprint();
    seahash(fingerprint.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> SystemState {
        SystemState::new(
            "install_neofetch".into(),
            "arch_linux".into(),
            "x86_64".into(),
            vec!["pacman".into(), "yay".into()],
            None,
            "zsh".into(),
        )
    }

    #[test]
    fn hash_is_deterministic() {
        let state = sample_state();
        let h1 = hash_state(&state);
        let h2 = hash_state(&state);
        assert_eq!(h1, h2, "same state must always produce the same hash");
    }

    #[test]
    fn hash_is_stable_across_clones() {
        let state = sample_state();
        let cloned = state.clone();
        assert_eq!(
            hash_state(&state),
            hash_state(&cloned),
            "cloned state must produce identical hash"
        );
    }

    #[test]
    fn different_intent_produces_different_hash() {
        let mut s1 = sample_state();
        let mut s2 = sample_state();
        s1.intent = "install_neofetch".into();
        s2.intent = "install_htop".into();
        assert_ne!(
            hash_state(&s1),
            hash_state(&s2),
            "different intents must produce different hashes"
        );
    }

    #[test]
    fn different_os_produces_different_hash() {
        let mut s1 = sample_state();
        let mut s2 = sample_state();
        s1.os = "arch_linux".into();
        s2.os = "macos".into();
        assert_ne!(hash_state(&s1), hash_state(&s2));
    }

    #[test]
    fn errors_do_not_affect_hash() {
        let mut s1 = sample_state();
        let s2 = sample_state();
        s1.errors_encountered.push("some error log".into());
        assert_eq!(
            hash_state(&s1),
            hash_state(&s2),
            "errors are runtime context, not cache keys — hash must be identical"
        );
    }
}
