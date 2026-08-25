# Contributing to Shovel

First off, thank you for considering contributing to Shovel! We welcome contributions from everyone, whether you're fixing a bug, adding a feature, improving documentation, or just asking a question.

This guide will help you get started. Please read through it before making your first contribution.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Development Environment](#development-environment)
- [Building and Running](#building-and-running)
- [Code Quality](#code-quality)
- [Testing](#testing)
- [Architecture Overview](#architecture-overview)
- [How to Contribute](#how-to-contribute)
- [Pull Request Process](#pull-request-process)
- [Code Review](#code-review)
- [Reporting Bugs](#reporting-bugs)
- [Feature Requests](#feature-requests)

## Code of Conduct

This project is governed by the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior by opening an issue on GitHub.

## Development Environment

Shovel is a Rust workspace. To set up your development environment:

1. **Install Rust** — Use [rustup](https://rustup.rs/). The project pins a **nightly** toolchain in [`rust-toolchain.toml`](rust-toolchain.toml); `rustup` will install the pinned channel automatically when you build.

2. **Install system dependencies** — On Linux, you'll need the following packages (names may vary by distribution):

   ```bash
   # Debian/Ubuntu
   sudo apt-get install libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libsqlite3-dev

   # Fedora
   sudo dnf install gtk3-devel webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel sqlite-devel
   ```

   On macOS and Windows, the Dioxus desktop dependencies are typically fewer — check the [Dioxus documentation](https://dioxuslabs.com/learn/0.7/getting_started/desktop) for details.

3. **Clone the repository**:

   ```bash
   git clone https://github.com/Fynth/Shovel.git
   cd Shovel
   ```

4. **Verify the setup** by running the project (see below).

## Building and Running

Build and run the desktop app:

```bash
cargo run -p app --features desktop
```

For a release build:

```bash
cargo build -p app --release --features desktop
```

To check that everything compiles without building:

```bash
cargo check --workspace
```

## Code Quality

Before submitting code, ensure it meets our quality standards:

### Formatting

All Rust code must be formatted with `rustfmt`:

```bash
cargo fmt --all
```

CI will fail if formatting is not up to date (`cargo fmt --all -- --check`).

### Linting

Run Clippy on the entire workspace:

```bash
cargo clippy --workspace --all-targets
```

CI enforces `-D warnings`, meaning Clippy warnings are treated as errors. Address all warnings before submitting a PR.

### Additional Configuration

We include a [`clippy.toml`](clippy.toml) at the project root — make sure your linting respects its settings.

## Testing

Run the full test suite before pushing:

```bash
cargo test --workspace
```

If you're adding new functionality, please add corresponding tests. For bug fixes, consider adding a regression test.

## Architecture Overview

Shovel is a modular Rust workspace. For a detailed understanding of the codebase, including crate responsibilities, state management patterns, and important design decisions, read [`AGENTS.md`](AGENTS.md).

For a complete walkthrough of the user interface — screens, panels, global state, and data flows — read [`docs/ui-description.md`](docs/ui-description.md). Read it before touching anything under `ui/`.

Key crates at a glance:

| Crate | Purpose |
|---|---|
| `app` | Desktop shell, Dioxus launch, crash reporting |
| `ui` | Dioxus UI, global state, workspace/connect screens |
| `models` | Shared domain models and settings |
| `storage` | Local persistence (connections, sessions, queries, chat) |
| `connection` | DB connection orchestration and SSH tunnels |
| `database` | Common driver traits and error types |
| `driver-*` | Backend-specific database drivers |
| `explorer` | Schema/database tree loading and metadata |
| `query-core` | Query execution and table editing |
| `query-format` | SQL formatting |
| `query-io` | Import/export logic |
| `query` | Facade re-exporting query sub-crates |
| `acp` | ACP runtime, permissions, terminals, Ollama bridge |
| `acp-registry` | ACP registry fetch and install |
| `services` | Facade for major operations |

## How to Contribute

We use a standard GitHub fork-and-pull workflow:

1. **Fork** the repository on GitHub.
2. **Create a branch** from `main` for your work:

   ```bash
   git checkout -b my-feature-branch
   ```

3. **Make your changes**, keeping commits small and focused. Write descriptive commit messages.
4. **Run the quality checks** before pushing:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets
   cargo test --workspace
   ```

5. **Push** your branch to your fork.
6. **Open a Pull Request** against the `main` branch of this repository.

### Commit Guidelines

- Use clear, concise commit messages that describe _what_ and _why_.
- Reference issues where applicable (e.g., `Fixes #123`).

## Pull Request Process

1. Ensure your PR description clearly explains the problem and solution.
2. If your PR adds a visible feature, consider including a screenshot or short screencast.
3. Link any related issues in the PR description.
4. Update documentation if your change affects public APIs, configuration, or workflows.
5. Make sure all CI checks pass (formatting, clippy, tests).
6. A maintainer will review your PR. Expect feedback and be open to discussion.

### What to Expect After Submitting

- A maintainer will typically respond within a few days.
- You may be asked to make changes — this is normal and part of the process.
- Once approved, a maintainer will merge your PR.

## Code Review

All submissions require review. We aim to be constructive and respectful in our reviews.

Reviewers will check for:

- Correctness and edge-case handling
- Test coverage
- Code clarity and maintainability
- Adherence to existing patterns (see [`AGENTS.md`](AGENTS.md))
- Proper error handling
- Security considerations

As a contributor, you should:

- Be open to feedback and suggestions.
- Respond to review comments in a timely manner.
- Update your branch if changes are requested.

## Reporting Bugs

Found a bug? Please open an issue on GitHub. Include as much detail as possible:

- Steps to reproduce
- Expected vs. actual behavior
- Screenshots or logs if applicable
- Your environment (OS, Shovel version, database type)

## Feature Requests

Feature requests are welcome! Open an issue describing:

- What you'd like to see added
- Why it would be useful
- Any implementation ideas you have

For larger features, consider starting a discussion before implementing — this can save effort and help shape the design.

## Getting Help

- Open an issue for bugs, questions, or feature requests.
- Check [`AGENTS.md`](AGENTS.md) for detailed architecture documentation.

Thank you for contributing to Shovel! 🚀
