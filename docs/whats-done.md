---
title: "Project Status"
description: "What's built, what's coming"
order: 4
---

### Currently Working

> **Scaffolded & CI-Ready** — The Rust project is initialized with module structure, tests (unit + integration), and CI/CD. The core abstraction types are built.

- **System Context Probing**: `sysinfo` and `which` automatically identify distros, shells, and package managers.
- **State-Aware Fingerprinting**: The exact machine intent + state is hashed cleanly and deterministically with `SeaHash`.
- **API Key & Auth Resolution Cascade**: `auth.rs` successfully fetches, selects, and rotates models across multiple LLM providers (BYOK paradigm), modeled after the `OpenClaw` framework.
- **Provider & Error Normalization**: Fallback strategies and standardized API endpoints mapped nicely to OpenAI, Anthropic, Gemini, Groq, OpenRouter, Mistral, xAI, etc. 
- **Cold Start Wizard**: Initial hooks are present for configuration prompts when no model options hit locally. 

### What's Next

- Building out the **4-Role LLM Pipeline** (Talker → Doer → Checker → Fixer).
- Caching logic and **Global Bloom Filters** with SQLite integration.
- The Interactive **CLI approval Diff UI** ("dry-runs").
- Network layers for the **Universal Registry** (community script lookups).

Check out [our source repo](https://github.com/Prakharprasun/ultimate-installer) directly for active updates.
