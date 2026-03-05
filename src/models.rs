//! Model selection and role-to-model mapping.
//!
//! Future: role-based model routing (Talker, Doer, Checker, Fixer)
//! and config JSON parsing. Inspired by OpenClaw's model-selection.ts.

/// The four pipeline roles that require LLM models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Talker,
    Doer,
    Checker,
    Fixer,
}

/// A reference to a specific provider + model combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

/// Parse a "provider/model" string into a ModelRef.
pub fn parse_model_ref(raw: &str) -> Option<ModelRef> {
    let (provider, model) = raw.split_once('/')?;
    let provider = provider.trim().to_string();
    let model = model.trim().to_string();
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some(ModelRef { provider, model })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_model_ref() {
        let r = parse_model_ref("openai/gpt-4.1-mini").unwrap();
        assert_eq!(r.provider, "openai");
        assert_eq!(r.model, "gpt-4.1-mini");
    }

    #[test]
    fn parse_invalid_model_ref_no_slash() {
        assert!(parse_model_ref("just-a-model").is_none());
    }

    #[test]
    fn parse_invalid_model_ref_empty_parts() {
        assert!(parse_model_ref("/model").is_none());
        assert!(parse_model_ref("provider/").is_none());
    }
}
