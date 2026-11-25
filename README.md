# Ultimate Installer (`ulti`)

> **Deterministic AI compiler for safe CLI installs.**  
> 🌐 Website: [https://ultimate-installer.vercel.app](https://ultimate-installer.vercel.app)

A **Rust-powered** CLI (`ulti`) that compiles explicit or natural-language setup requests into a canonical, versioned `InstallSpec` (SHA-256 hashed), checks what is already installed on the machine, previews the exact OS package-manager command, requires explicit approval, verifies the installed binaries, and records verified runs in a local SQLite cache.

---

## Quick Start

### Option A: Download Prebuilt Binaries (macOS & Ubuntu)

Download the latest archive from [GitHub Releases](https://github.com/Prakharprasun/ultimate-installer/releases/latest):

```bash
# macOS (Apple Silicon)
curl -fsSLO https://github.com/Prakharprasun/ultimate-installer/releases/latest/download/ulti-macos-arm64.tar.gz
tar xzf ulti-macos-arm64.tar.gz && chmod +x ulti && ./ulti "install ripgrep and fd" --dry-run

# Ubuntu (Linux x86_64)
curl -fsSLO https://github.com/Prakharprasun/ultimate-installer/releases/latest/download/ulti-linux-x86_64.tar.gz
tar xzf ulti-linux-x86_64.tar.gz && chmod +x ulti && ./ulti "install ripgrep and fd" --dry-run
```

### Option B: Build & Run from Source

#### Prerequisites
- Rust toolchain (`cargo`, `rustc` — Rust 2024 edition)
- **macOS**: [Homebrew](https://brew.sh) (`brew` on `PATH`)
- **Ubuntu**: `apt-get` (and `sudo` when running as a non-root user)
- *(Windows compiles cleanly and reports the supported install platforms.)*

```bash
git clone https://github.com/Prakharprasun/ultimate-installer.git
cd ultimate-installer
cargo build --release

# Preview without making changes (no API key needed):
./target/release/ulti "install ripgrep and fd" --dry-run

# Run interactively with approval prompt and version verification:
./target/release/ulti "install ripgrep and fd"

# Or run directly via cargo:
cargo run -- "install ripgrep and fd" --dry-run
```

### 30-Second Ubuntu Demo (`rg` Already Installed)

On an Ubuntu machine where `rg` (`ripgrep`) is already present, asking for both `ripgrep` and `fd` detects `rg`, proposes **only** Ubuntu's `fd-find` package, requires approval, and verifies `fdfind`:

```text
$ ulti "install ripgrep and fd"
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

### Supported Catalog (Current Build)

| Tool ID | Aliases | macOS (Homebrew) | Ubuntu (`apt-get`) | Notes |
| --- | --- | --- | --- | --- |
| `ripgrep` | `ripgrep`, `rg` | pkg `ripgrep`, bin `rg` | pkg `ripgrep`, bin `rg` | Verified via `rg --version` |
| `fd` | `fd`, `fdfind`, `fd-find` | pkg `fd`, bin `fd` | pkg `fd-find`, bin `fdfind` | On Ubuntu, run `fdfind` (or `alias fd=fdfind`) per [upstream fd docs](https://github.com/sharkdp/fd#installation) |

### Descriptive Requests with OpenAI (Optional)

Explicit requests like `ulti "install ripgrep and fd"` run **100% locally** with no API key.

For descriptive requests such as:

```bash
export OPENAI_API_KEY="sk-..."
./target/release/ulti "Set me up with ripgrep for searching text and fd for finding files"
```

- Uses OpenAI's documented **Structured Outputs** (`response_format: { type: "json_schema" }`) with default tested model **`gpt-4o-mini`** (overridable via `OPENAI_MODEL`).
- The model may only return canonical tool IDs from the local catalog (`ripgrep`, `fd`) or `unsupported`. All responses are strictly validated in Rust (`deny_unknown_fields`); unsupported/mixed requests produce zero installs.
- `OPENAI_API_KEY` is sent only to OpenAI's HTTPS endpoint, redacted in debug output, and stripped from all child process environments.
- See [docs/api-keys.md](./docs/api-keys.md) for details.

### Verification & Tests

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

---

## Roadmap (Planned for Future Iterations)

The architecture below describes the longer-term vision. The **shared Universal Registry / Bloom filter cache**, **four-model pipeline (Talker → Doer → Checker → Fixer)**, **automatic multi-key rotation**, and **additional providers (Anthropic, Gemini, Groq, OpenRouter, Ollama)** are on the roadmap and are **not** active in the current build. See [docs/whats-done.md](./docs/whats-done.md) for the exact current status.

---

## Core Philosophy

| Principle | Description |
| --- | --- |
| **Talker vs. Doer** | Separate intelligence (LLM) from execution (Rust). The LLM generates logic; the Rust binary enforces safety and handles the shell. |
| **State-Aware Hashing** | Scripts are mapped to a composite hash of `Intent + OS + Arch + Package Manager + Hardware`, not just a package name. |
| **Zero-Cost Latency** | A local Bloom filter check determines if a verified script exists before a single network packet is sent or a single LLM token is spent. |
| **Universal Caching** | Without it, this is no better than asking ChatGPT. Every user-approved script is uploaded to a shared registry so the entire community benefits. |

---

## Multi-Model Pipeline

The system uses **four specialised LLM roles**, each with a single responsibility:

| Role | Responsibility |
| --- | --- |
| **Talker** | Takes the user's natural language, cleans the input, and prompt-engineers a precise request for the Doer. |
| **Doer** | Generates a bash or Python script to accomplish the task. Never executes directly — always produces reviewable code. |
| **Checker** | Previews what the script will change (a "diff" of side-effects). Routes to the Doer on user approval, or to the Fixer on failure. |
| **Fixer** | Takes the error logs (user-provided or system-captured) and feeds them back to the Doer with full context for a retry. |

**Model routing** happens internally — the right model is selected per role based on complexity.

---

## Model Configuration (BYOK)

Ultimate Installer is **Bring Your Own Key** — we don't host models or proxy API calls. You provide your own API keys, and the CLI talks directly to the provider. This design is inspired by [OpenClaw's](https://github.com/Prakharprasun/openclaw) provider architecture.

### Setting API Keys

Keys are configured via environment variables. Set at least one:

```bash
# In your shell profile, .env file, or ~/.ultimate-installer/config.env
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
GEMINI_API_KEY=...
OPENROUTER_API_KEY=sk-or-...
GROQ_API_KEY=...
```

### Key Resolution Cascade

Inspired by OpenClaw's `resolveApiKeyForProvider` pattern, keys are resolved in priority order:

1. **Process env var** (e.g., `OPENAI_API_KEY` in current shell)
2. **Project `.env` file** (`./.env` in current directory)
3. **User config file** (`~/.ultimate-installer/config.env`)
4. **Config JSON override** (`~/.ultimate-installer/config.json` → `providers.openai.apiKey`)

The first non-empty value wins. Existing process env vars are never overridden by file-based config.

### API Key Rotation

Following OpenClaw's `executeWithApiKeyRotation` pattern: if a provider returns a rate-limit error, the CLI automatically rotates to the next configured key before falling back to a different provider entirely.

```bash
# Multiple keys for the same provider (comma-separated)
OPENAI_API_KEYS=sk-key1,sk-key2,sk-key3
```

### Role-to-Model Mapping

Each pipeline role can be mapped to a different provider/model. Defaults are sensible, but overridable:

```json
// ~/.ultimate-installer/config.json
{
  "models": {
    "talker": "openai/gpt-4.1-mini",
    "doer": "anthropic/claude-sonnet-4-5",
    "checker": "openai/gpt-4.1-mini",
    "fixer": "anthropic/claude-sonnet-4-5"
  }
}
```

The Talker and Checker are lightweight roles (fast, cheap models). The Doer and Fixer need high reasoning (use the best model you can afford).

### Local Models

If you run [Ollama](https://ollama.com) locally, the CLI can auto-discover available models — no API key needed, no tokens spent:

```bash
# The CLI detects Ollama at its default address
OLLAMA_BASE_URL=http://localhost:11434
```

### Provider-Agnostic Design

Internally, all LLM calls go through a provider-agnostic interface. The CLI normalises provider IDs (`openai`, `anthropic`, `google`, `openrouter`, `ollama`, etc.) and resolves the correct auth mode, base URL, and model ID for each. Adding a new provider means adding one entry to the provider map — no pipeline changes needed.

### Cold Start

If no API keys are configured and Ollama isn't running, the CLI shouldn't just crash with a cryptic error. Instead, it launches a **first-run wizard**:

```
⚠ No model providers configured.

Run `ulti config` to get started, or set an API key:
  export OPENAI_API_KEY=sk-...
  export ANTHROPIC_API_KEY=sk-ant-...

Or install Ollama for free local models:
  https://ollama.com
```

The goal: zero-confusion cold start. A new user should go from `cargo install` to a working `ulti` in under 60 seconds.

---

## Execution Pipeline

The system operates on a **"Fail-Forward"** pipeline to maximise efficiency and minimise API costs:

```
User Input ─► Context Probe ─► Intent Normalization ─► Bloom Gate
                                                          │
                                        ┌─── HIT ────────┤──── MISS ───────┐
                                        ▼                                   ▼
                               Universal Lookup                      LLM Generation
                                        │                                   │
                                        └──────────┐       ┌───────────────┘
                                                   ▼       ▼
                                              Human-in-the-Loop
                                                   │
                                           ┌── Approve ──┐
                                           ▼              ▼
                                     Execute Script    Cache & Upload
                                           │
                                     ┌── Failure ──┐
                                     ▼              ▼
                                   Fixer ──► Doer (retry)
```

### Pipeline Steps

1. **Context Probe** — The CLI probes the local environment (detects `pacman` on Arch, `brew` on macOS, GPU drivers, etc.).
2. **Intent Normalization** — Natural language is cleaned into a structured intent (e.g., "Get me neofetch" → `Action: install, Target: neofetch`).
3. **Bloom Gate** — A local bit-array check confirms if a verified script for this *exact system state* exists in the Universal Cache.
4. **Universal Lookup** — On a Bloom hit, the signed script is pulled from the registry.
5. **LLM Generation (Fallback)** — On a Bloom miss, the Talker → Doer pipeline generates a custom script.
6. **Human-in-the-Loop** — The user reviews the script diff. On approval, the script is cached locally and uploaded to the Universal Registry.
7. **Self-Healing** — If execution fails, logs are piped to the Fixer, which provides context for the Doer to generate a fix-up script.

---

## The State-Aware Fingerprint

A simple slug like `pkg_install:neofetch` is too fragile. "Arch Linux with an outdated keyring" and "macOS with a broken Homebrew path" need entirely different scripts. The solution: hash the machine's full **"DNA"** as the lookup key.

```rust
struct SystemState {
    intent: String,                  // e.g., "install_neofetch"
    os: String,                      // e.g., "arch_linux"
    arch: String,                    // e.g., "x86_64"
    package_managers: Vec<String>,   // e.g., ["pacman", "yay"]
    gpu_driver: Option<String>,      // Crucial for gaming (LoL, Wine, etc.)
    shell: String,                   // e.g., "zsh"
    errors_encountered: Vec<String>, // Starts empty; the Fixer appends error logs on each retry
}
```

The Bloom filter doesn't just check for "neofetch" — it checks for *"neofetch on Arch using pacman for an x86_64 user running zsh."*

### Context Engine

Uses the `sysinfo` and `which` crates to gather system telemetry in milliseconds:

- **Package Manager Detection** — Loop through known binaries (`apt`, `brew`, `pacman`, `dnf`, `nix`, `yay`).
- **Environment Variables** — Check `$PATH` and `$SHELL`.
- **Hardware Specs** — Detect M-series Macs, NVIDIA GPUs, etc.

### Hashing Flow

1. **User Input**: "Get me neofetch."
2. **Cleaner**: Maps "Get me" → `install`.
3. **Fingerprinter**: Combines `install` + `neofetch` + `arch` + `pacman` → composite string.
4. **Hash**: `SeaHash` → `1a2b3c4d...`
5. **Bloom Check**: If the hash exists in the filter → a script exists for *this exact environment*.

### Handling "Dirty" States

> [!NOTE]
> **This is a known hard problem, not a selling point.** Detecting that `wine-stable` conflicts with a script expecting `wine-staging` is essentially re-implementing dependency resolution — something `pacman` and `apt` already do poorly. We'll probably do it poorly too.

The state includes what's *already installed*. We do a **best-effort** check: the Cleaner returns a list of expected prerequisites, and if obvious conflicts are detected, the cached script is skipped in favour of a fresh LLM generation pass. But this will miss edge cases.

The real safety net here is the **Fixer loop** — if a cached script fails because of an undetected dependency conflict, the error logs feed back into the Doer for a retry with full context. We're not trying to solve dependency resolution; we're trying to recover gracefully when it bites us.

---

## Features

- **Environment Detection** — Automatically identifies distros, shells, and missing dependencies.
- **Safety Reviews** — Never executes code without showing a diff of what will change.
- **Self-Healing** — Failed scripts are retried via the Fixer → Doer loop with full error context.
- **Offline First** — Common tasks are handled entirely through the local SQLite cache.
- **Universal Package Support** — Can use Homebrew, distro-specific managers (`pacman`, `apt`, `dnf`), or even `curl`/browser downloads for things like League of Legends.

---

## Technical Stack

### Core

| Component | Technology | Purpose |
| --- | --- | --- |
| **Language** | Rust | Memory safety, speed, and static binaries |
| **Logic Gate** | Bloom Filter | Probabilistic local lookup for global cache hits |
| **Database** | SQLite | Local storage for verified scripts |
| **CLI Engine** | Clap | Type-safe argument parsing and UI |
| **Networking** | Reqwest + Tokio | Asynchronous registry fetches |

### Internal Components

| Component | Responsibility | Rust Crate |
| --- | --- | --- |
| **Telemetry** | Probing OS, Arch, and installed binaries | `sysinfo`, `which` |
| **Normalizer** | Turning "I want X" into `Action: X, Target: Y` | `mistral-rs` (local) or `reqwest` (API) |
| **Hasher** | Creating the state-aware Bloom filter key | `seahash` |
| **Gatekeeper** | The local `.bin` bit-array check | `bloomfilter` |

---

## Why This Works

By making the cache key **state-specific**, we prevent the "it works on my machine" problem. An Arch user and a Fedora user produce different hashes and pull different scripts from the Universal Cache. The Bloom filter ensures we only hit the network when we're confident a script exists for our specific flavour of Linux or macOS.

---

## Trust Model & Security

The Universal Cache is the hardest problem in this project. Who runs the registry? How do you prevent malicious scripts from being uploaded? A signed script from one user's machine doesn't inherently mean it's safe for yours.

### Current Approach: Human-in-the-Loop for Everything

The same review step that gates LLM-generated scripts **also gates registry-pulled scripts**. Every script — whether freshly generated or pulled from the Universal Cache — is shown to the user as a full diff before execution. The user must explicitly approve it.

This means we are **not blindly trusting the registry**. The cache is a performance optimisation (skip the LLM call), not a trust bypass.

> [!CAUTION]
> **Registry scripts are community-contributed and NOT audited.** Always read the script diff before approving execution. The Universal Cache accelerates lookups, but the final trust decision is always yours. Never approve a script you haven't reviewed.

### Future Trust Layers (Not Yet Implemented)

These are potential improvements to layer on top of human review as the registry scales:

- **Community Votes** — Scripts with more successful runs and upvotes surface higher.
- **Cryptographic Author Identity** — Signed scripts tied to verified contributor accounts.
- **Sandboxed Test Runs** — Registry-side CI that executes scripts in disposable containers before marking them as "verified."
- **Reputation Scoring** — Contributors build trust over time; new/unknown authors get flagged.

For now, the **user is the final gatekeeper**, and the CLI makes that unavoidable by design.

---

## Implementation Gotchas

Things to get right when scaffolding the Rust binary — these will silently break the pipeline if ignored.

### TTY Persistence

When the Doer runs a script (like a `curl | bash` installer), the Rust `Command` must spawn with `.stdout(Stdio::inherit())`. Without this, interactive installers that prompt for a password or a `[Y/n]` confirmation will **hang silently** in the background with no visible output.

```rust
Command::new("bash")
    .arg(&script_path)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
```

### The "Dry Run" Diff

For the Checker role, use the `similar` crate to generate terminal-colored diffs. Showing the user exactly which lines of their `.zshrc` or files in `/etc/` will change is the strongest way to build the trust described in the Security section. This is the "diff of side-effects" that makes human review actually useful — without it, the user is just reading a raw script and hoping.

### Atomic Caching

When a user hits "Yes" on the happiness check, the **local SQLite write must happen before the network upload**. If the user loses internet mid-upload, they should still benefit from their own successful run locally. The flow is:

1. User approves → write to local SQLite **first**
2. Background async upload to Universal Registry
3. If the upload fails, the script is still cached locally — retry the upload on next CLI invocation

---

## Reference Architecture: OpenClaw

The provider and model configuration system is heavily informed by [OpenClaw](https://github.com/Prakharprasun/openclaw) (MIT licensed — compatible with our AGPL-3.0). We're not copy-pasting TypeScript — we're **translating the logic into idiomatic Rust**.

> [!NOTE]
> We link to [our fork](https://github.com/Prakharprasun/openclaw) rather than the official OpenClaw repo. The official repo moves fast — by pinning to a fork, we ensure the code we're referencing matches the patterns described in this README.

### TS → Rust Translation Map

| TypeScript | Rust Equivalent |
| --- | --- |
| `interface` / `type` | `struct` + `trait` |
| `async` / `await` | `async` / `.await` (Tokio runtime) |
| Optional chaining (`?.`) | `Option<T>` + `.map()` / `if let` |
| Object spreads (`{...a, ...b}`) | Struct update syntax or builder pattern |
| `Record<string, T>` | `HashMap<String, T>` |
| `try / catch` | `Result<T, E>` + `?` operator |
| Implicit `null` returns | Forced explicit `Option::None` |

### Key OpenClaw Files to Study

These files contain the logic we're porting:

| File | What to Take | Rust Module |
| --- | --- | --- |
| `model-auth.ts` | Key resolution cascade, env var lookup, provider normalization | `src/auth.rs` |
| `api-key-rotation.ts` | Key rotation loop on rate-limit errors | `src/auth.rs` |
| `models-config.providers.ts` | Provider definitions, base URLs, model catalogs | `src/providers.rs` |
| `model-selection.ts` | Provider ID normalization, model ref parsing | `src/models.rs` |
| `model-fallback.ts` | Fallback candidate list, retry with different models | `src/fallback.rs` |

### Why Rust Will Be Cleaner

Rust forces explicit handling of every error case that TypeScript papers over. OpenClaw's `resolveApiKeyForProvider` has a long cascade of `if (result) return result` checks — in Rust, this becomes a natural chain of `Option` combinators or early `?` returns. The logic is the same; the Rust version just won't have hidden `undefined` surprises.

---

## Project FAQ & Overview

### Describe what it does in 50 characters or less.
Deterministic AI compiler for safe CLI installs.

### URL
https://ultimate-installer.vercel.app

### Describe the product and what it does or will do.
Ultimate Installer (`ulti`) is a single-binary Rust CLI that turns explicit or natural-language setup requests into safe, deterministic, verified package installations across macOS and Linux.

**What works today:** `ulti` compiles a request into a versioned canonical `InstallSpec` (hashed with SHA-256), probes the host OS, package manager, and already-installed binaries, plans only what is missing, previews the exact command (`--dry-run` supported), requires explicit user approval, executes directly via Rust's `Command` with API keys stripped from child environments, verifies every binary with `--version`, and records verified runs in a persistent local SQLite cache. Descriptive natural-language requests resolve through OpenAI Structured Outputs (`gpt-4o-mini`) into canonical catalog tool IDs—models never generate or execute arbitrary shell commands—and verified descriptive requests replay locally with zero API calls.

**Where it is going:** Expanding the verified tool catalog and OS coverage, then layering a shared community registry (with Bloom-filter fast lookups and state-aware caching) and a multi-model self-healing loop for complex, multi-step developer environments.

### How far along are you?
We have a working, tested Rust CLI (`ulti`) with macOS (Homebrew) and Ubuntu (`apt-get`) support, a canonical `InstallSpec` + SHA-256 deterministic hashing layer, a persistent local SQLite verification cache, an OpenAI Structured Outputs adapter for descriptive requests, 68 passing automated tests (unit + CLI boundary + mock provider), and cross-platform GitHub Actions CI/CD building downloadable macOS and Ubuntu binaries. We are now running live user sessions with developers to expand the catalog based on real setup requests.

### How long have you been working on this? How much of that has been full-time?
I have been building the core architecture and Rust implementation for **7 months part-time (~20 hours/week)** alongside my coursework and machine learning research at **BITS Pilani** and **NTNU**. The foundational work on deterministic AI code compilation started **11 months ago** during the Google DeepMind Hackathon with [NeuroSmith](https://www.kaggle.com/competitions/gemini-3/writeups/neurosmith-a-deterministic-multimodal-neural-netw) ([GitHub](https://github.com/Prakharprasun/neurosmith-architect/)).

### What tech stack are you using, or planning to use, to build this product? Include AI models and AI coding tools you use.
- **Core CLI (shipped):** Rust (`clap` for CLI parsing, `tokio` and `reqwest` for async HTTPS provider calls, `sysinfo` and `which` for OS/hardware/binary probing, `serde`/`serde_json` + `sha2` for canonical `InstallSpec` serialization and SHA-256 hashing, bundled `rusqlite` for the persistent local SQLite cache, and `seahash` for machine fingerprinting).
- **AI models:** Shipped with OpenAI `gpt-4o-mini` via strict Structured Outputs (`json_schema`) to map descriptive requests strictly to canonical catalog IDs. Planned additions include additional providers (Anthropic, Gemini, Groq, local Ollama) and higher-reasoning models for complex multi-step setup scripts and self-healing retries.
- **Planned infrastructure:** Shared community registry with Redis/Bloom-filter fast lookups and `similar`-powered diff previews for file edits.
- **Development tools:** Neovim, VS Code, PyCharm, Antigravity IDE, and GitHub Copilot.

### Do you have revenue?
No — Ultimate Installer is open source (AGPL-3.0) and free to use while we validate developer adoption and expand the verified catalog.

### Why did you pick this idea to work on? Do you have domain expertise in this area? How do you know people need what you're making?
I have spent the past year building deterministic AI developer tools. In December 2025, I placed in the **Top 50 out of 4,097 global submissions** in the **Google DeepMind Gemini 3 Pro Hackathon** (winning **\$10,000 in API credits**) for **NeuroSmith**, a multimodal studio that compiles sketches, controls, and voice input into a strict `MODEL_SPEC` to generate runnable, hallucination-free PyTorch and TensorFlow code.

Through building NeuroSmith, contributing bug fixes to Google's open-source **JAX** library, and running ML experiments across macOS laptops and Linux clusters at **BITS Pilani** and **NTNU**, I repeatedly hit the same bottleneck: writing code is no longer what slows teams down—getting tools, system packages, and environments to install cleanly across machines is. Developers routinely lose hours to OS-specific package-name mismatches (like `fd` on macOS vs. `fd-find`/`fdfind` on Ubuntu), partial installs, and fragile copy-pasted scripts. NeuroSmith proved that wrapping LLMs inside strict, deterministic specs earns developer trust; Ultimate Installer brings that exact compiler architecture to machine setup.

### Who are your competitors? What do you understand about your business that they don't?
Our competitors fall into two camps: **terminal AI assistants** (such as Warp AI and GitHub Copilot CLI) and **traditional environment/package managers** (such as Homebrew, Nix, and Docker).

Terminal AI chatbots treat installation as open-ended text generation: every user burns tokens generating an unverified shell snippet from scratch that often breaks on their specific OS, architecture, or partial machine state. Traditional tools like Nix are deterministic and reproducible, but impose a steep configuration-language learning curve.

The insight from NeuroSmith is that developers do not want raw LLM shell generation—they want **deterministic compilation with human-in-the-loop verification**. Ultimate Installer separates intent resolution from execution: inputs compile into a canonical `InstallSpec` keyed to the machine's actual OS, architecture, and package manager, only missing items are planned, and every verified install is cached locally (and eventually across a shared registry) so once a setup works, nobody has to spend tokens or debug that install again.

### Have you taken any investment yet?
No.

### Are you currently fundraising?
No.

---

## Status

> **Working Demo + Local SQLite Cache Built** — Canonical `InstallSpec` + SHA-256 deterministic hashing, persistent local SQLite verification cache, explicit local install workflow (`ripgrep` and `fd`) on macOS (Homebrew) and Ubuntu (`apt-get`), `--dry-run`, explicit approval, post-install binary verification, OpenAI Structured Outputs (`gpt-4o-mini`) for descriptive requests, 68 passing tests, and GitHub Actions CI/Release pipelines. Shared registry, Redis/Bloom filter, four-model pipeline, automatic key rotation, and additional providers remain on the roadmap.

- 📦 Repo: [github.com/Prakharprasun/ultimate-installer](https://github.com/Prakharprasun/ultimate-installer)
- 📜 License: [AGPL-3.0](./LICENSE)