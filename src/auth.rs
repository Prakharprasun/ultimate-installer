//! API key resolution and rotation.
//!
//! BYOK (Bring Your Own Key) — resolves API keys from env vars, config files,
//! and rotates on rate-limit errors. Inspired by OpenClaw's model-auth.ts.

use std::collections::HashMap;

/// Supported LLM providers and their expected env var names.
pub fn provider_env_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("openai", "OPENAI_API_KEY");
    m.insert("anthropic", "ANTHROPIC_API_KEY");
    m.insert("google", "GEMINI_API_KEY");
    m.insert("openrouter", "OPENROUTER_API_KEY");
    m.insert("groq", "GROQ_API_KEY");
    m.insert("mistral", "MISTRAL_API_KEY");
    m.insert("xai", "XAI_API_KEY");
    m
}

/// Result of resolving an API key.
#[derive(Debug, Clone)]
pub struct ResolvedKey {
    pub api_key: String,
    pub source: String,
}

/// Resolve an API key for a given provider using the cascade:
/// 1. Process env var
/// 2. (Future: .env file)
/// 3. (Future: user config file)
/// 4. (Future: config JSON)
///
/// Returns None if no key is found.
pub fn resolve_api_key(provider: &str) -> Option<ResolvedKey> {
    let env_map = provider_env_map();
    let env_var = env_map.get(provider)?;

    std::env::var(env_var).ok().and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(ResolvedKey {
                api_key: trimmed,
                source: format!("env: {env_var}"),
            })
        }
    })
}

/// Check if any provider has a configured API key.
pub fn has_any_api_key() -> bool {
    provider_env_map()
        .keys()
        .any(|provider| resolve_api_key(provider).is_some())
}

/// Collect all configured keys for a provider (supports comma-separated lists).
/// Used for key rotation on rate-limit errors.
pub fn collect_provider_keys(provider: &str) -> Vec<String> {
    let env_map = provider_env_map();
    let Some(base_var) = env_map.get(provider) else {
        return Vec::new();
    };

    let mut keys = Vec::new();

    // Check single-key var
    if let Ok(val) = std::env::var(base_var) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            keys.push(trimmed.to_string());
        }
    }

    // Check plural var (e.g., OPENAI_API_KEYS=sk-1,sk-2)
    let plural_var = format!("{base_var}S");
    if let Ok(val) = std::env::var(&plural_var) {
        for key in val.split(',') {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() && !keys.contains(&trimmed) {
                keys.push(trimmed);
            }
        }
    }

    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_env_map_has_expected_entries() {
        let map = provider_env_map();
        assert_eq!(map.get("openai"), Some(&"OPENAI_API_KEY"));
        assert_eq!(map.get("anthropic"), Some(&"ANTHROPIC_API_KEY"));
        assert_eq!(map.get("google"), Some(&"GEMINI_API_KEY"));
    }

    #[test]
    fn resolve_api_key_returns_none_for_unknown_provider() {
        assert!(resolve_api_key("nonexistent_provider").is_none());
    }

    #[test]
    fn has_any_api_key_returns_bool() {
        // Just assert it doesn't panic. Actual value depends on env.
        let _result = has_any_api_key();
    }

    #[test]
    fn collect_provider_keys_returns_empty_for_unknown() {
        let keys = collect_provider_keys("nonexistent_provider");
        assert!(keys.is_empty());
    }
}
