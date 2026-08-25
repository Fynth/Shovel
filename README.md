<p align="center">
  <img src="app/assets/icon.svg" alt="Shovel logo" width="96" height="96" />
</p>

<h1 align="center">Shovel</h1>

<p align="center">
  <a href="https://github.com/Fynth/Shovel/actions/workflows/test.yml"><img src="https://github.com/Fynth/Shovel/actions/workflows/test.yml/badge.svg" alt="Test"></a>
  <a href="https://github.com/Fynth/Shovel/actions/workflows/supply-chain.yml"><img src="https://github.com/Fynth/Shovel/actions/workflows/supply-chain.yml/badge.svg" alt="Supply Chain"></a>
  <a href="https://github.com/Fynth/Shovel/releases"><img src="https://img.shields.io/github/v/release/Fynth/Shovel?sort=semver" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Fynth/Shovel" alt="License: MIT"></a>
</p>

<img width="1890" height="1137" alt="Shovel workspace screenshot" src="https://github.com/user-attachments/assets/2584a85c-1d82-4a8c-ab61-d4a477a32e11" />

<p align="center">
  A native desktop database client built with Rust and Dioxus.
  <br />
  Persistent database chat threads, fast query workflows, and AI-assisted SQL through ACP.
</p>

<p align="center">
  <strong>SQLite</strong> • <strong>PostgreSQL</strong> • <strong>MySQL</strong> • <strong>ClickHouse</strong> • Rust • Dioxus • ACP • Ollama
</p>

<p align="center">
  If Shovel saves you time, give the repo a star. ⭐
</p>

---

## Table of Contents

- [Why Shovel](#why-shovel)
- [What It Can Do](#what-it-can-do)
- [Database Support](#database-support)
- [AI / ACP Support](#ai--acp-support)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Project Layout](#project-layout)
- [What Makes It Different](#what-makes-it-different)
- [Current Status](#current-status)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

---

## Why Shovel

Most database clients are either heavy, web-first, or overloaded with enterprise UI noise.

Shovel is trying to be the opposite:

- native desktop app, not a browser tab pretending to be one
- chat-first database workflows with persistent threads
- fast for daily SQL work
- clear workspace with explorer, editor, results, history, and saved queries
- direct support for local AI agents through ACP
- modular Rust workspace instead of a monolith

It is built for people who want a responsive database tool that feels closer to an editor than to a dashboard.

## What It Can Do

| Area | What you get |
| --- | --- |
| Chat | Persistent database chat threads, context-aware prompts, SQL generation, streaming responses, agent-guided execution |
| Querying | Multi-tab SQL editor, query history, structure tabs, formatting, pagination |
| AI Autocomplete | Inline SQL completions (Zed-style), multi-provider (DeepSeek + CodeStral), streaming token display |
| Data exploration | Connection explorer, schemas/databases, tables, views, column loading |
| Result workflows | Sort, filter, inspect rows, JSON view, row details |
| Editing | Draft inserts, cell edits, deletes, apply/discard changes for editable table views |
| Import / export | CSV import, CSV/JSON/XLSX export |
| AI workflows | ACP agent panel, OpenCode via ACP registry, embedded Ollama/DeepSeek ACP bridges |
| UX | Compact desktop layout, dark/light theme, saved queries/snippets, tooltips |

## Database Support

| Database | Connect | Explore | Query | Export | Edit table rows |
| --- | --- | --- | --- | --- | --- |
| SQLite | Yes | Yes | Yes | Yes | Yes |
| PostgreSQL | Yes | Yes | Yes | Yes | Yes |
| MySQL | Yes | Yes | Yes | Yes | Yes |
| ClickHouse | Yes | Yes | Yes | Yes | Not yet |

## AI / ACP Support

Shovel includes an ACP client layer, embedded ACP bridges for DeepSeek and Ollama, and inline AI SQL autocompletion.

That means you can:

- connect an external ACP-compatible coding/database agent over `stdio`
- install and connect supported ACP registry agents such as OpenCode
- spawn an embedded DeepSeek or Ollama-backed ACP agent directly from the UI
- generate SQL against the active connection context
- send general database prompts and insert generated SQL into the editor
- get **inline SQL completions** as you type — just like Zed's inline assistant
  - Tab to accept, keep typing to ignore
  - Streaming tokens appear in real-time
  - Multi-provider fallback (DeepSeek → CodeStral)
  - Schema-aware: completions know your tables and columns

This is opt-in. If you do not care about AI features, Shovel still works as a regular database client.

## Quick Start

### Requirements

- **Rust nightly** (pinned in [`rust-toolchain.toml`](rust-toolchain.toml))
- for desktop builds: system dependencies required by Dioxus Desktop/WebView on your platform
- on Windows for raw `.exe`: Microsoft Edge WebView2 Runtime

### Run the desktop app

```bash
cargo run -p app --features desktop
```

### Build a release binary

```bash
cargo build -p app --release --features desktop
```

### Build a Linux release package artifact

GitHub Actions can build a Linux desktop tarball from `.github/workflows/linux.yml`.

The archive contains:

- `bin/shovel`
- desktop entry
- app icon
- README

### Build a Windows bundle

```bash
dx bundle --release --platform desktop --package app --features bundle --package-types msi
```

## Installation

Release artifacts are published here:

- [GitHub Releases](https://github.com/Fynth/Shovel/releases)

### Ubuntu / Debian via APT repository

If the APT repository has already been configured on the machine, installation is just:

```bash
sudo apt update
sudo apt install shovel
```

To add the repository first:

```bash
echo "deb [arch=amd64 trusted=yes] https://fynth.github.io/Shovel/apt stable main" | sudo tee /etc/apt/sources.list.d/shovel.list
sudo apt update
sudo apt install shovel
```

Notes:

- this currently targets `amd64`
- the repository is currently unsigned, so the source line uses `trusted=yes`

### Ubuntu / Debian via downloaded `.deb`

Download the latest Debian package from:

- [Latest Releases](https://github.com/Fynth/Shovel/releases/latest)

Then install it with:

```bash
sudo apt install ./shovel_<version>_amd64.deb
```

or:

```bash
sudo dpkg -i shovel_<version>_amd64.deb
sudo apt -f install
```

### Arch Linux / EndeavourOS / Manjaro via AUR

Available AUR packages:

- `shovel`
- `shovel-bin`
- `shovel-git`

Install with:

```bash
yay -S shovel
```

or:

```bash
yay -S shovel-bin
```

or:

```bash
yay -S shovel-git
```

### Linux via Flatpak bundle

Download the Flatpak bundle from:

- [Latest Releases](https://github.com/Fynth/Shovel/releases/latest)

Then install and run it with:

```bash
flatpak install --user ./shovel-linux-x86_64.flatpak
flatpak run dev.shovel.app
```

### Linux via release tarball

Download the Linux archive from:

- [Latest Releases](https://github.com/Fynth/Shovel/releases/latest)

Then unpack and run:

```bash
tar -xzf shovel-linux-x86_64.tar.gz
./bin/shovel
```

The archive contains:

- `bin/shovel`
- `lib/shovel/assets/app.css`
- desktop entry
- app icon

### Windows

Download from:

- [Latest Releases](https://github.com/Fynth/Shovel/releases/latest)

Available artifacts:

- `shovel-windows-x86_64.exe`
- Windows `.msi` installer

Notes:

- for raw `.exe`, Microsoft Edge WebView2 Runtime is required
- `.msi` is the better option for end-user installation

### Build from source

Requirements:

- Rust nightly
- platform desktop dependencies required by Dioxus Desktop/WebView

Run directly:

```bash
cargo run -p app --features desktop
```

Build release binary:

```bash
cargo build -p app --release --features desktop
```

## Project Layout

Shovel is organized as a Rust workspace with focused crates instead of one large application crate.

| Crate | Responsibility |
| --- | --- |
| `app` | desktop launcher, build pipeline, embedded ACP agent entrypoint |
| `ui` | Dioxus desktop frontend, AI autocomplete, chat |
| `models` | shared domain models and settings |
| `services` | unified facade — single entry point for `ui` into all lower crates |
| `connection` / `connection-ssh` | connection orchestration and SSH support |
| `explorer` | schema and object discovery |
| `query` | query execution, pagination, editable rows, formatting, import/export |
| `acp-core` / `acp` / `acp-registry` | ACP runtime, DeepSeek/Ollama agents, registry integration |
| `driver-*` | database-specific implementations (SQLite, PostgreSQL, MySQL, ClickHouse) |
| `storage` | local persistence for settings, sessions, history, saved queries, chat |

## What Makes It Different

- Native UI with Rust and Dioxus instead of Electron
- AI integration built around ACP instead of a one-off prompt box
- Inline SQL autocomplete like Zed — multi-provider, streaming, schema-aware
- Editable result grid for real table workflows, not just read-only browsing
- Workspace split into independent crates, which makes the codebase easier to evolve
- Feature-gated database drivers — build only what you need

## Current Status

Shovel is actively evolving.

Today it is already useful for:

- running queries quickly
- exploring local and remote databases
- editing SQLite/PostgreSQL/MySQL table data
- exporting/importing common formats
- using ACP-powered assistants for SQL generation (DeepSeek, Ollama, OpenCode)
- AI inline SQL autocompletion with streaming responses

Areas that still need expansion:

- deeper ClickHouse edit workflows
- broader packaging polish across platforms
- more agent presets and richer ACP UX

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — layered architecture and dependency rules
- [`AGENTS.md`](AGENTS.md) — repo operating manual and conventions
- [`docs/ui-description.md`](docs/ui-description.md) — full description of the UI, screens, panels, and data flows
- [`CHANGELOG.md`](CHANGELOG.md) — release history
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to contribute
- [`SECURITY.md`](SECURITY.md) — how to report vulnerabilities

## Contributing

Issues, UX feedback, database-specific bugs, and performance reports are useful.

If you open a bug report, include:

- database type and version
- the query or workflow that failed
- expected behavior
- actual behavior
- platform (`Linux`, `macOS`, `Windows`)

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full contribution guide.

## License

Shovel is released under the [MIT License](LICENSE).
