---
title: "API Key Setup"
description: "Connect your LLM provider"
order: 3
---

Ultimate Installer uses a **Bring Your Own Key** (BYOK) architecture. We do not proxy API calls. You provide the keys, and the CLI talks directly to the provider of your choice.

### 1. Set Your Keys

Set your API key via environment variables in your shell profile (e.g., `~/.bashrc` or `~/.zshrc`), a local `.env` file, or the global `~/.ultimate-installer/config.env` file. 

Set at least one to get started:

```bash
export OPENAI_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
export GEMINI_API_KEY=...
export OPENROUTER_API_KEY=sk-or-...
export GROQ_API_KEY=...
```

### 2. Using Local Models

Want to run everything offline and free? Perfect. The CLI auto-discovers models running via [Ollama](https://ollama.com). No token costs. 

```bash
export OLLAMA_BASE_URL=http://localhost:11434
```

### 3. Key Cascade and Failover

The CLI automatically handles **Key Resolution Cascade** (choosing the first available config level) and **API Key Rotation**. If an API key is rate-limited, the system seamlessly attempts your next provided key or falls back to an alternative provider.
