---
title: "Install"
description: "Download prebuilt binaries or build Ultimate Installer from source"
order: 2
---

### Option A: Download Prebuilt Binaries (macOS & Ubuntu)

Download the latest release archive directly from [GitHub Releases](https://github.com/Prakharprasun/ultimate-installer/releases/latest):

```bash
# macOS (Apple Silicon)
curl -fsSLO https://github.com/Prakharprasun/ultimate-installer/releases/latest/download/ulti-macos-arm64.tar.gz
tar xzf ulti-macos-arm64.tar.gz && chmod +x ulti && ./ulti "install ripgrep and fd" --dry-run

# macOS (Intel)
curl -fsSLO https://github.com/Prakharprasun/ultimate-installer/releases/latest/download/ulti-macos-x86_64.tar.gz
tar xzf ulti-macos-x86_64.tar.gz && chmod +x ulti && ./ulti "install ripgrep and fd" --dry-run

# Ubuntu (Linux x86_64)
curl -fsSLO https://github.com/Prakharprasun/ultimate-installer/releases/latest/download/ulti-linux-x86_64.tar.gz
tar xzf ulti-linux-x86_64.tar.gz && chmod +x ulti && ./ulti "install ripgrep and fd" --dry-run
```

### Option B: Build from Source

#### Prerequisites

- [Rust toolchain](https://rustup.rs/) (`cargo`, `rustc` — Rust 2024 edition)
- **macOS**: [Homebrew](https://brew.sh) (`brew` on `PATH`)
- **Ubuntu**: `apt-get` (and `sudo` when running as a non-root user)
- **Windows**: Compiles cleanly and explains the currently supported install platforms.

```bash
git clone https://github.com/Prakharprasun/ultimate-installer.git
cd ultimate-installer
cargo build --release
```

### 30-Second Ubuntu Demo (`rg` Already Installed)

When `rg` (`ripgrep`) is already present on Ubuntu, asking for both `ripgrep` and `fd` detects `rg`, proposes **only** Ubuntu's `fd-find` package, asks for approval, and verifies `fdfind`:

```text
$ ./ulti "install ripgrep and fd"
Platform: Ubuntu (apt-get)

Already installed:
  ✓ ripgrep (rg) — ripgrep 14.1.0

Missing (will install):
  • fd -> package 'fd-find' (executable: 'fdfind')
    Note: On Ubuntu, the binary is installed as 'fdfind' (due to a Debian binary name collision with fdclone). You can run it with `fdfind` or create an alias (`alias fd=fdfind`).

Command to run:
  sudo apt-get install -y fd-find

Proceed with installation? [y/N]: y
Running: sudo apt-get install -y fd-find

Verification:
  ✓ fd (fdfind) — fdfind 9.0.0
  ✓ ripgrep (rg) — ripgrep 14.1.0

All requested tools are installed and verified.
```

### Descriptive Requests (Optional OpenAI Setup)

```bash
export OPENAI_API_KEY="sk-..."
./ulti "Set me up with ripgrep for searching text and fd for finding files"
```

### Supported Catalog

| Tool | Aliases | macOS (Homebrew) | Ubuntu (`apt-get`) |
| --- | --- | --- | --- |
| `ripgrep` | `ripgrep`, `rg` | package `ripgrep`, binary `rg` | package `ripgrep`, binary `rg` |
| `fd` | `fd`, `fdfind`, `fd-find` | package `fd`, binary `fd` | package `fd-find`, binary `fdfind` |

Check [API Key Setup](/api-keys) for provider configuration and [Project Status](/whats-done) for the roadmap.
