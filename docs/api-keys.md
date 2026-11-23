---
title: "API Key Setup"
description: "Connect your LLM provider"
order: 3
---

Ultimate Installer uses a **Bring Your Own Key** (BYOK) architecture. We do not proxy API calls. You provide the key, and the CLI talks directly to the provider's HTTPS endpoint.

### 1. Explicit Requests Need No API Key

Exact catalog requests run completely locally without any API key:

```bash
ulti "install ripgrep and fd"
ulti "install ripgrep and fd" --dry-run
```

### 2. OpenAI Setup (for Descriptive Requests)

To use ordinary-language requests such as:

```bash
ulti "Set me up with ripgrep for searching text and fd for finding files"
```

export your OpenAI API key:

```bash
export OPENAI_API_KEY="sk-..."
```

- **Default / Tested Model**: `gpt-4o-mini` (via OpenAI's Structured Outputs `response_format: { type: "json_schema" }` interface).
- **Optional Model Override**: Set `OPENAI_MODEL` (for example `export OPENAI_MODEL="gpt-4o-mini"`).
- **Security Guarantees**:
  - `OPENAI_API_KEY` is sent only to OpenAI's HTTPS endpoint (`https://api.openai.com/v1/chat/completions`).
  - The key is redacted in debug output (`[REDACTED]`).
  - Provider API keys are stripped from all child process environments (`brew`, `apt-get`, `sudo`, and verification commands).
  - The model may only return canonical tool IDs from the local catalog or `unsupported`; model-generated commands, flags, URLs, or package-manager names are never accepted or executed.

### 3. Roadmap (Not Yet Wired in This Build)

The following provider features remain on the roadmap:

- Additional providers (`ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENROUTER_API_KEY`, `GROQ_API_KEY`) and local Ollama auto-discovery
- Automatic multi-key rotation and cross-provider failover
- File-based key cascade (`.env`, `~/.ultimate-installer/config.env`, `config.json`)
