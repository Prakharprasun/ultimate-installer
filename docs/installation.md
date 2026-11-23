---
title: "Install"
description: "Build and run Ultimate Installer from source"
order: 2
---

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (`cargo`, `rustc` — Rust 2024 edition)
- **macOS**: [Homebrew](https://brew.sh) (`brew` on `PATH`)
- **Ubuntu**: `apt-get` (and `sudo` when running as a non-root user)
- **Windows**: Compiles cleanly and explains the currently supported install platforms.

### Build from Source

```bash
git clone https://github.com/Prakharprasun/ultimate-installer.git
cd ultimate-installer
cargo build --release
```

### Usage

Preview the installation plan without making any changes (no API key required):

```bash
./target/release/ulti "install ripgrep and fd" --dry-run
```

Run interactively with approval prompt and post-install version verification:

```bash
./target/release/ulti "install ripgrep and fd"
```

Repeat the same command once installed — `ulti` detects both binaries and exits cleanly with nothing to install.

### Descriptive Requests (Optional OpenAI Setup)

```bash
export OPENAI_API_KEY="sk-..."
./target/release/ulti "Set me up with ripgrep for searching text and fd for finding files"
```

### Supported Catalog

| Tool | Aliases | macOS (Homebrew) | Ubuntu (`apt-get`) |
| --- | --- | --- | --- |
| `ripgrep` | `ripgrep`, `rg` | package `ripgrep`, binary `rg` | package `ripgrep`, binary `rg` |
| `fd` | `fd`, `fdfind`, `fd-find` | package `fd`, binary `fd` | package `fd-find`, binary `fdfind` |

> **Ubuntu note:** Per upstream `fd` packaging, Ubuntu installs the binary as `fdfind` (due to a Debian naming collision with `fdclone`). `ulti` verifies `fdfind --version` and reminds you which command name to type.

Check [API Key Setup](/api-keys) for provider configuration and [Project Status](/whats-done) for the roadmap.
