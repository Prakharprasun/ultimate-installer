//! Model fallback and retry logic.
//!
//! When a primary model fails (rate limit, timeout, context overflow),
//! the fallback system tries alternative models before giving up.
//! Inspired by OpenClaw's model-fallback.ts.

/// Placeholder — will implement fallback candidate resolution and retry loop.
/// The core pattern from OpenClaw:
/// 1. Try the primary model
/// 2. On rate-limit/timeout, rotate API keys first
/// 3. If all keys exhausted, try fallback models in order
/// 4. If all models exhausted, return the last error
pub fn placeholder() {
    // TODO: implement
}
