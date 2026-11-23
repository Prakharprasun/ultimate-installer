//! OpenAI structured-output provider adapter for descriptive install requests.
//!
//! Uses OpenAI's documented `response_format` JSON Schema interface to map natural-language
//! requests strictly onto canonical catalog tool IDs, or an `unsupported` result.
//! Never accepts or executes model-generated commands, flags, URLs, or package-manager names.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::auth::{self, ResolvedKey};
use crate::catalog::{self, CatalogEntry};

/// Default OpenAI Chat Completions HTTPS endpoint.
pub const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

/// Tested default OpenAI model supporting Structured Outputs (`json_schema`).
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

/// Default HTTP request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 20;

/// Validated decision returned by the model provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderDecision {
    /// Supported install intent mapped strictly to canonical catalog entries.
    Install(Vec<&'static CatalogEntry>),
    /// Request is unsupported or mixes supported and unsupported tools (produces zero installs).
    Unsupported { reason: String },
}

/// Distinct, actionable errors from the model provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    MissingApiKey,
    InsecureEndpoint(String),
    RejectedCredentials(String),
    RateLimited(String),
    NetworkFailure(String),
    InvalidOutput(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::MissingApiKey => write!(
                f,
                "OPENAI_API_KEY is not set.\nFor descriptive requests, export OPENAI_API_KEY and re-run your command.\nFor explicit requests (no API key needed), run: ulti \"install ripgrep and fd\""
            ),
            ProviderError::InsecureEndpoint(msg) => write!(
                f,
                "Refusing to send OPENAI_API_KEY to non-HTTPS endpoint: {msg}"
            ),
            ProviderError::RejectedCredentials(msg) => write!(
                f,
                "OpenAI rejected your credentials ({msg}). Check OPENAI_API_KEY and re-run to retry."
            ),
            ProviderError::RateLimited(msg) => write!(
                f,
                "OpenAI rate limit reached ({msg}). Wait a moment and re-run to retry."
            ),
            ProviderError::NetworkFailure(msg) => write!(
                f,
                "Network error while contacting OpenAI ({msg}). Check your connection and re-run to retry."
            ),
            ProviderError::InvalidOutput(msg) => write!(
                f,
                "Provider returned invalid or unsafe output ({msg}). No changes were made; re-run to retry."
            ),
        }
    }
}

impl std::error::Error for ProviderError {}

/// OpenAI client for resolving descriptive natural-language requests.
pub struct OpenAiClient {
    api_key: ResolvedKey,
    endpoint: String,
    model: String,
    timeout: Duration,
    http: reqwest::Client,
}

impl fmt::Debug for OpenAiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiClient")
            .field("api_key", &self.api_key) // ResolvedKey redacts the secret
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl OpenAiClient {
    /// Build an OpenAI client from environment variables (`OPENAI_API_KEY`, optional `OPENAI_MODEL`,
    /// optional `OPENAI_BASE_URL`). Enforces HTTPS unless `ULTI_ALLOW_INSECURE_LOCAL_PROVIDER=1`
    /// is explicitly set for local loopback test doubles.
    pub fn from_env() -> Result<Self, ProviderError> {
        let api_key = auth::resolve_api_key("openai").ok_or(ProviderError::MissingApiKey)?;
        let model = std::env::var("OPENAI_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string());
        let endpoint = std::env::var("OPENAI_BASE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_ENDPOINT.to_string());
        let allow_local_http = std::env::var("ULTI_ALLOW_INSECURE_LOCAL_PROVIDER")
            .ok()
            .as_deref()
            == Some("1");

        Self::with_config(
            api_key,
            endpoint,
            model,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            allow_local_http,
        )
    }

    /// Build an OpenAI client with explicit configuration (useful for local test doubles).
    pub fn with_config(
        api_key: ResolvedKey,
        endpoint: String,
        model: String,
        timeout: Duration,
        allow_local_http: bool,
    ) -> Result<Self, ProviderError> {
        validate_endpoint(&endpoint, allow_local_http)?;
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ProviderError::NetworkFailure(e.to_string()))?;

        Ok(Self {
            api_key,
            endpoint,
            model,
            timeout,
            http,
        })
    }

    /// Send the user's descriptive request and supported catalog to OpenAI using Structured Outputs,
    /// then strictly validate the response in Rust.
    pub async fn resolve_intent(
        &self,
        user_request: &str,
    ) -> Result<ProviderDecision, ProviderError> {
        let payload = build_request_payload(&self.model, user_request);

        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::NetworkFailure(format!(
                        "request timed out after {}s",
                        self.timeout.as_secs()
                    ))
                } else {
                    ProviderError::NetworkFailure(e.to_string())
                }
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| ProviderError::NetworkFailure(e.to_string()))?;

        match status.as_u16() {
            200..=299 => {}
            401 | 403 => {
                return Err(ProviderError::RejectedCredentials(format!(
                    "HTTP {}",
                    status.as_u16()
                )));
            }
            429 => {
                return Err(ProviderError::RateLimited(format!(
                    "HTTP {}",
                    status.as_u16()
                )));
            }
            code => {
                return Err(ProviderError::NetworkFailure(format!("HTTP {code}")));
            }
        }

        let completion: ChatCompletionResponse = serde_json::from_str(&body)
            .map_err(|e| ProviderError::InvalidOutput(format!("malformed API envelope: {e}")))?;

        let first_choice =
            completion.choices.into_iter().next().ok_or_else(|| {
                ProviderError::InvalidOutput("missing choices in response".into())
            })?;

        if let Some(refusal) = first_choice.message.refusal
            && !refusal.trim().is_empty()
        {
            return Err(ProviderError::InvalidOutput(format!(
                "model refused request: {refusal}"
            )));
        }

        let content = first_choice.message.content.ok_or_else(|| {
            ProviderError::InvalidOutput("missing message content in response".into())
        })?;

        validate_model_json(&content)
    }
}

fn validate_endpoint(endpoint: &str, allow_local_http: bool) -> Result<(), ProviderError> {
    if endpoint.starts_with("https://") {
        return Ok(());
    }
    if allow_local_http
        && (endpoint.starts_with("http://127.0.0.1") || endpoint.starts_with("http://localhost"))
    {
        return Ok(());
    }
    Err(ProviderError::InsecureEndpoint(endpoint.to_string()))
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    response_format: serde_json::Value,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    refusal: Option<String>,
}

/// Strict schema for the model's JSON payload. Any unexpected field is rejected.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModelIntent {
    intent: String,
    tools: Vec<String>,
    reason: String,
}

fn build_request_payload(model: &str, user_request: &str) -> ChatCompletionRequest {
    let catalog_lines: Vec<String> = catalog::all_entries()
        .iter()
        .map(|entry| {
            format!(
                "- id: \"{}\" (aliases: {}): {}",
                entry.id,
                entry.aliases.join(", "),
                entry.description
            )
        })
        .collect();

    let system_prompt = format!(
        "You are an intent classifier for a software installer CLI.\n\
         Supported tool catalog:\n{}\n\n\
         Rules:\n\
         1. Return intent=\"install\" ONLY if the user wants to install one or more tools from the supported catalog AND does NOT request any unsupported tool.\n\
         2. Return canonical tool IDs from the catalog in `tools` (e.g. \"ripgrep\", \"fd\").\n\
         3. If the user asks for any tool not in the catalog, mixes supported and unsupported tools, or does not ask to install tools, return intent=\"unsupported\", tools=[], and explain briefly in `reason`.\n\
         4. Never output shell commands, flags, URLs, or package-manager names.",
        catalog_lines.join("\n")
    );

    let response_format = serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "install_intent_response",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "intent": {
                        "type": "string",
                        "enum": ["install", "unsupported"]
                    },
                    "tools": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "reason": {
                        "type": "string"
                    }
                },
                "required": ["intent", "tools", "reason"],
                "additionalProperties": false
            }
        }
    });

    ChatCompletionRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: "system",
                content: system_prompt,
            },
            ChatMessage {
                role: "user",
                content: user_request.to_string(),
            },
        ],
        response_format,
        temperature: 0.0,
    }
}

/// Strictly validate the model's structured JSON output before it can reach the planner.
pub fn validate_model_json(raw_json: &str) -> Result<ProviderDecision, ProviderError> {
    let parsed: RawModelIntent = serde_json::from_str(raw_json)
        .map_err(|e| ProviderError::InvalidOutput(format!("schema mismatch: {e}")))?;

    match parsed.intent.as_str() {
        "unsupported" => {
            let reason = if parsed.reason.trim().is_empty() {
                "Request is not supported by the current catalog.".to_string()
            } else {
                parsed.reason.trim().to_string()
            };
            Ok(ProviderDecision::Unsupported { reason })
        }
        "install" => {
            if parsed.tools.is_empty() {
                return Err(ProviderError::InvalidOutput(
                    "intent was 'install' but tool list was empty".into(),
                ));
            }
            let mut entries: Vec<&'static CatalogEntry> = Vec::new();
            for tool_id in &parsed.tools {
                // Require exact canonical ID (no flags, URLs, package managers, or unknown items).
                let Some(entry) = catalog::find_by_id(tool_id) else {
                    return Err(ProviderError::InvalidOutput(format!(
                        "model returned unsupported or non-canonical tool ID '{tool_id}'"
                    )));
                };
                if entry.id != tool_id {
                    return Err(ProviderError::InvalidOutput(format!(
                        "model returned non-canonical tool ID '{tool_id}'"
                    )));
                }
                if !entries.iter().any(|existing| existing.id == entry.id) {
                    entries.push(entry);
                }
            }
            Ok(ProviderDecision::Install(entries))
        }
        other => Err(ProviderError::InvalidOutput(format!(
            "unexpected intent '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_canonical_install_response() {
        let json = r#"{"intent":"install","tools":["ripgrep","fd"],"reason":"User wants text search and file finder"}"#;
        let decision = validate_model_json(json).unwrap();
        assert_eq!(
            decision,
            ProviderDecision::Install(vec![
                catalog::find_by_id("ripgrep").unwrap(),
                catalog::find_by_id("fd").unwrap(),
            ])
        );
    }

    #[test]
    fn validates_unsupported_response() {
        let json = r#"{"intent":"unsupported","tools":[],"reason":"htop is not in the catalog"}"#;
        let decision = validate_model_json(json).unwrap();
        assert_eq!(
            decision,
            ProviderDecision::Unsupported {
                reason: "htop is not in the catalog".to_string()
            }
        );
    }

    #[test]
    fn rejects_unexpected_field() {
        let json = r#"{"intent":"install","tools":["ripgrep"],"reason":"ok","command":"rm -rf /"}"#;
        assert!(matches!(
            validate_model_json(json),
            Err(ProviderError::InvalidOutput(_))
        ));
    }

    #[test]
    fn rejects_mixed_or_unknown_tool_ids() {
        let json = r#"{"intent":"install","tools":["ripgrep","htop"],"reason":"mixed"}"#;
        assert!(matches!(
            validate_model_json(json),
            Err(ProviderError::InvalidOutput(_))
        ));
    }

    #[test]
    fn rejects_non_https_remote_endpoint() {
        let key = ResolvedKey {
            api_key: "sk-test-123".into(),
            source: "test".into(),
        };
        let err = OpenAiClient::with_config(
            key,
            "http://example.com/v1/chat/completions".into(),
            DEFAULT_OPENAI_MODEL.into(),
            Duration::from_secs(5),
            true,
        )
        .unwrap_err();
        assert!(matches!(err, ProviderError::InsecureEndpoint(_)));
    }

    #[test]
    fn debug_output_redacts_api_key() {
        let secret = "sk-fake-secret-9999999999";
        let key = ResolvedKey {
            api_key: secret.into(),
            source: "test".into(),
        };
        let client = OpenAiClient::with_config(
            key,
            DEFAULT_OPENAI_ENDPOINT.into(),
            DEFAULT_OPENAI_MODEL.into(),
            Duration::from_secs(5),
            false,
        )
        .unwrap();
        let dbg = format!("{client:?}");
        assert!(!dbg.contains(secret));
        assert!(dbg.contains("[REDACTED]"));
    }
}
