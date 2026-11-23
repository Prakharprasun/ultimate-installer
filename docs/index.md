---
title: "Ultimate Installer"
description: "Your terminal, but it understands English."
order: 0
---

A **Rust-powered** CLI (`ulti`) that turns explicit or natural-language setup requests into safe, reviewed, verified package installations.

### What Works Today

- **macOS (Homebrew) & Ubuntu (`apt-get`)**: Checks both the OS and the real package-manager executable before doing anything.
- **Explicit Local Mode (No API Key)**: `ulti "install ripgrep and fd"` runs 100% locally, checks what's already installed, shows exact OS package names (`fd-find` / `fdfind` on Ubuntu, `fd` on macOS), and previews the exact command.
- **Safety by Default**: `--dry-run` previews without changes; interactive installs require explicit `y`/`yes` approval, run Rust's `Command` directly, strip API keys from child environments, and verify every installed binary with `--version`.
- **Descriptive Mode (OpenAI Structured Outputs)**: With `OPENAI_API_KEY`, requests like `ulti "Set me up with ripgrep for searching text and fd for finding files"` map through `gpt-4o-mini` into the exact same validated catalog plan — never model-generated shell commands.

See [Install](/installation) to build and run from source, or check [Project Status](/whats-done) for what's shipped vs. on the roadmap.
