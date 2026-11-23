---
title: "How It Works"
description: "Safe planning, explicit approval, and binary verification"
order: 1
---

## Current Architecture (Working Build)

Every install request goes through a deterministic, human-approved Rust pipeline — **models never generate or execute shell commands**.

1. **Local Explicit Parser**: Requests naming supported catalog tools (such as `ulti "install ripgrep and fd"`) are parsed and validated locally with zero network calls and no API key.
2. **Optional OpenAI Structured Outputs**: Descriptive requests (such as `"Set me up with ripgrep for searching text and fd for finding files"`) are sent with the supported catalog to OpenAI (`gpt-4o-mini`) using strict `json_schema` output. The Rust client validates every field (`deny_unknown_fields`) and only accepts canonical catalog tool IDs or `unsupported`.
3. **Platform & State Probe**: Checks the OS and required executable (`brew` on macOS, `apt-get` + `sudo` on Ubuntu), probes whether each requested tool already works via `--version`, and plans **only the missing items**.
4. **Preview & Explicit Approval**: Prints what's already installed, what's missing, OS package names, and the exact command vector (`--dry-run` stops here). Execution requires explicit `y`/`yes` — empty input, EOF, or `n` aborts with zero changes.
5. **Direct Execution & Verification**: Runs the exact previewed command via Rust's `std::process::Command` with provider API keys stripped from child environments and inherited stdio for OS password prompts. Afterward, every requested binary is verified with `--version`.

---

## Roadmap: Fail-Forward Multi-Model Pipeline

Planned for future iterations beyond the verified catalog demo:

- **Four-Model Roles** (*Talker → Doer → Checker → Fixer*) for general-purpose shell workflows beyond curated package catalogs.
- **State-Aware Fingerprinting & Global Bloom Filter**: Hashing `Intent + OS + Arch + Package Managers + Shell` via `SeaHash` (already scaffolded in `src/context.rs` and `src/hash.rs`) to look up community-verified scripts in a shared **Universal Registry** with local SQLite caching.
- **Multi-Provider & Key Rotation**: Expanding beyond OpenAI to Anthropic, Gemini, Groq, OpenRouter, and local Ollama models.
