//! Provider definitions and configuration.
//!
//! Maps provider IDs to base URLs, model catalogs, and auth modes.
//! Future: this will be the Rust equivalent of OpenClaw's models-config.providers.ts.

/// Normalise a provider ID to its canonical form.
/// e.g., "OpenAI" -> "openai", "Google" -> "google"
pub fn normalize_provider_id(provider: &str) -> String {
    provider.trim().to_lowercase().replace(' ', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_whitespace_and_lowercases() {
        assert_eq!(normalize_provider_id("  OpenAI  "), "openai");
        assert_eq!(normalize_provider_id("Google"), "google");
        assert_eq!(normalize_provider_id("Open Router"), "open-router");
    }
}
