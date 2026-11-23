---
title: "Project Status"
description: "What's built, what's coming"
order: 4
---

### Currently Working

> **Working Demo Ready** — End-to-end install workflow on macOS (Homebrew) and Ubuntu (`apt-get`), with local explicit parsing, OpenAI structured-output intent resolution, `--dry-run`, explicit approval, and post-install binary verification.

- **Verified Tool Catalog (`src/catalog.rs`)**:
  - `ripgrep` → package `ripgrep`, executable `rg` (macOS & Ubuntu).
  - `fd` → macOS Homebrew package `fd` (executable `fd`); Ubuntu `apt-get` package `fd-find` (executable `fdfind`, with upstream Debian/Ubuntu naming guidance).
- **Local Explicit Parser (`src/parser.rs`)**: Handles explicit install requests (e.g. `ulti "install ripgrep and fd"`) with zero API calls and validates the entire request before planning.
- **Deterministic Planner & Safe Executor (`src/planner.rs`, `src/executor.rs`)**:
  - Checks both OS and required package-manager binary (`brew` on macOS, `apt-get` and `sudo` when non-root on Ubuntu; compiles on Windows with clear supported-platform messaging).
  - Inspects existing tool versions and plans only missing packages.
  - Previews the exact command vector (`--dry-run` supported) and requires explicit `y`/`yes` approval (empty input, EOF, or `n` aborts with zero changes).
  - Runs `std::process::Command` directly with inherited stdio for OS password prompts and strips provider API keys from child environments.
  - Verifies installed binaries with `--version` after execution and reports partial failures cleanly for safe retry.
- **OpenAI Structured-Output Adapter (`src/openai.rs`)**:
  - Resolves descriptive requests (e.g. `"Set me up with ripgrep for searching text and fd for finding files"`) using `gpt-4o-mini` and strict `json_schema` structured outputs.
  - Validates the entire response in Rust (`deny_unknown_fields`, canonical catalog IDs only) before handing off to the same deterministic planner.
  - Redacts `OPENAI_API_KEY` in debug output, enforces HTTPS, sets a request timeout, and distinguishes missing key, rejected credentials, rate limit, network failure, and invalid output.
- **System Context & Hashing (`src/context.rs`, `src/hash.rs`)**: OS/arch/package-manager probing and deterministic `SeaHash` fingerprinting.

### On the Roadmap (Next Iterations)

- Shared **Universal Registry** and community script cache with SQLite + Bloom filter lookups.
- **Four-Model Pipeline** (Talker → Doer → Checker → Fixer) for arbitrary shell workflows.
- **Automatic API Key Rotation** and multi-provider failover (Anthropic, Gemini, Groq, OpenRouter, Ollama).
- Interactive `ulti --config` setup wizard and Windows package-manager support.

Check out [our source repo](https://github.com/Prakharprasun/ultimate-installer) directly for active updates.
