//! Local parser for explicit install requests naming supported catalog entries.
//!
//! Keeps explicit requests like `"install ripgrep and fd"` completely local with zero LLM calls,
//! while routing descriptive natural-language requests to the configured model provider.

use crate::catalog::{self, CatalogEntry};

/// Outcome of attempting to parse a user request locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalParseOutcome {
    /// Explicit install request where every named tool is in the supported catalog.
    ExplicitInstall(Vec<&'static CatalogEntry>),
    /// Explicit install request containing one or more unsupported package names.
    /// Produces zero installs — never silently drops unknown packages.
    UnsupportedExplicit { unknown: Vec<String> },
    /// Empty or malformed request with no actionable content.
    InvalidRequest(String),
    /// Descriptive natural-language request that should be resolved by a model provider.
    NeedsProvider,
}

/// Words that indicate a descriptive natural-language sentence rather than an explicit package list.
const DESCRIPTIVE_HINT_WORDS: &[&str] = &[
    "for",
    "with",
    "to",
    "the",
    "a",
    "an",
    "me",
    "my",
    "something",
    "tool",
    "tools",
    "search",
    "searching",
    "find",
    "finding",
    "files",
    "text",
    "please",
    "can",
    "you",
    "help",
    "need",
    "want",
    "setup",
    "set",
    "up",
    "like",
    "alternative",
];

/// Parse a user request locally without calling an LLM.
///
/// Validates the entire request before returning `ExplicitInstall`.
pub fn parse_local_request(raw: &str) -> LocalParseOutcome {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LocalParseOutcome::InvalidRequest("Request cannot be empty.".to_string());
    }

    // Strip trailing sentence punctuation such as "." or "!"
    let cleaned = trimmed.trim_end_matches(['.', '!', '?']).trim();
    let lower = cleaned.to_ascii_lowercase();

    // Check for explicit "install ..." prefix
    let rest = if let Some(after) = lower.strip_prefix("install ") {
        after.trim()
    } else if lower == "install" {
        return LocalParseOutcome::InvalidRequest(
            "No packages specified after 'install'. Try: ulti \"install ripgrep and fd\""
                .to_string(),
        );
    } else {
        // Does not start with "install " — treat as a descriptive request for the model provider.
        return LocalParseOutcome::NeedsProvider;
    };

    if rest.is_empty() {
        return LocalParseOutcome::InvalidRequest(
            "No packages specified after 'install'. Try: ulti \"install ripgrep and fd\""
                .to_string(),
        );
    }

    // Tokenize by commas, '&', '+', and whitespace, treating standalone "and" as a separator.
    let raw_tokens: Vec<String> = rest
        .replace([',', '&', '+'], " ")
        .split_whitespace()
        .map(|t| t.trim_matches(['"', '\'', '(', ')']).to_string())
        .filter(|t| !t.is_empty())
        .collect();

    if raw_tokens.is_empty() {
        return LocalParseOutcome::InvalidRequest(
            "No packages specified after 'install'.".to_string(),
        );
    }

    // Filter out conjunction "and" when it's used as a separator between items.
    let item_tokens: Vec<String> = raw_tokens
        .iter()
        .filter(|t| t.as_str() != "and")
        .cloned()
        .collect();

    if item_tokens.is_empty() {
        return LocalParseOutcome::InvalidRequest(
            "No packages specified after 'install'.".to_string(),
        );
    }

    // If the user wrote a descriptive sentence starting with "install" (e.g.
    // "install a fast tool for searching text"), route it to the provider instead of
    // rejecting descriptive words as unknown package names.
    let has_descriptive_words = item_tokens
        .iter()
        .any(|t| DESCRIPTIVE_HINT_WORDS.contains(&t.as_str()));
    if has_descriptive_words {
        return LocalParseOutcome::NeedsProvider;
    }

    let mut matched: Vec<&'static CatalogEntry> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();

    for token in &item_tokens {
        if let Some(entry) = catalog::find_by_alias(token) {
            if !matched.iter().any(|existing| existing.id == entry.id) {
                matched.push(entry);
            }
        } else {
            unknown.push(token.clone());
        }
    }

    if !unknown.is_empty() {
        return LocalParseOutcome::UnsupportedExplicit { unknown };
    }

    LocalParseOutcome::ExplicitInstall(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_ripgrep_and_fd() {
        let outcome = parse_local_request("install ripgrep and fd");
        assert_eq!(
            outcome,
            LocalParseOutcome::ExplicitInstall(vec![
                catalog::find_by_id("ripgrep").unwrap(),
                catalog::find_by_id("fd").unwrap(),
            ])
        );
    }

    #[test]
    fn parses_aliases_and_deduplicates() {
        let outcome = parse_local_request("Install rg, fd-find, and ripgrep!");
        assert_eq!(
            outcome,
            LocalParseOutcome::ExplicitInstall(vec![
                catalog::find_by_id("ripgrep").unwrap(),
                catalog::find_by_id("fd").unwrap(),
            ])
        );
    }

    #[test]
    fn rejects_mixed_supported_and_unsupported_explicit() {
        let outcome = parse_local_request("install ripgrep and htop");
        assert_eq!(
            outcome,
            LocalParseOutcome::UnsupportedExplicit {
                unknown: vec!["htop".to_string()]
            }
        );
    }

    #[test]
    fn rejects_fully_unsupported_explicit() {
        let outcome = parse_local_request("install neofetch");
        assert_eq!(
            outcome,
            LocalParseOutcome::UnsupportedExplicit {
                unknown: vec!["neofetch".to_string()]
            }
        );
    }

    #[test]
    fn routes_descriptive_request_to_provider() {
        let outcome = parse_local_request(
            "Set me up with ripgrep for searching text and fd for finding files",
        );
        assert_eq!(outcome, LocalParseOutcome::NeedsProvider);

        let outcome2 = parse_local_request("install a fast tool for searching text");
        assert_eq!(outcome2, LocalParseOutcome::NeedsProvider);
    }

    #[test]
    fn rejects_empty_or_bare_install() {
        assert!(matches!(
            parse_local_request("   "),
            LocalParseOutcome::InvalidRequest(_)
        ));
        assert!(matches!(
            parse_local_request("install"),
            LocalParseOutcome::InvalidRequest(_)
        ));
    }
}
