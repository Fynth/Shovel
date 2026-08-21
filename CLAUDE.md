# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Shovel is a native desktop database client built as a Rust workspace with Dioxus Desktop 0.7.3.
For product surface and install instructions see `README.md`. For the full operating manual
and repo-specific rules see `AGENTS.md` — it is authoritative and should be preferred over
generic framework advice. For the layered architecture and dependency rules see `ARCHITECTURE.md`.

## Toolchain and CI

- **Toolchain is nightly** (pinned in `rust-toolchain.toml`); crates use `edition = "2024"`.
- CI (`.github/workflows/test.yml`) gates merges on:
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo audit
  ```
- `cargo-deny` config (`deny.toml`) denies unknown registries and git wildcards; licenses are
  restricted to an allow-list. `multiple-versions` is currently `warn`, not deny.
- `rustfmt.toml` sets `max_width = 100`, `imports_granularity = "Crate"`, and
  `reorder_modules = false` — do not let an editor reorder module items.

## Common commands

```bash
cargo run -p app --features desktop            # run the desktop app (default features include desktop)
cargo build -p app --release --features desktop
cargo check --workspace                        # fast compile check across all crates
cargo test --workspace                          # full test suite
cargo test -p <crate> <test_name>               # single test (e.g. cargo test -p models settings::)
cargo fmt --all                                 # apply formatting (CI checks with --check)
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                                      # dependency vulnerability scan
cargo deny check                                 # licenses + bans + sources
```

Feature flags worth knowing: `desktop` (default on `app`) and `web` select Dioxus targets;
`bundle` enables Windows `.msi` packaging; `acp`'s `embedding` feature pulls in `ort`/`tokenizers`
for the sqlite-vec semantic prompt cache.

## Dependency layer rules (do not violate)

`ARCHITECTURE.md` defines four layers. The non-obvious, enforced boundaries:

- **`ui` may only import `models` and `services`.** It must not depend on `connection`,
  `explorer`, `query`, `storage`, or `acp` directly — all operational calls go through the
  `services` facade. Never reach into a `driver-*` crate from `ui`.
- **`driver-*` crates must not depend on `ui`, `app`, `services`, `models`, or ACP code.** They
  implement the `DatabaseDriver` trait from `database` and translate `sqlx`/HTTP errors to `String`.
- **`services` is a thin re-export facade.** New operational functions belong in their domain crate
  first (`connection`, `explorer`, `query`, `storage`, `acp`), then get re-exported from `services`.
  Note: `services` exists but the rest of the workspace does not yet consistently route through it
  for every call — `ui` still couples to lower crates in places. Don't assume a refactor has completed.
- **`query` is itself a facade** over `query-core`, `query-format`, and `query-io`. Work that looks
  like it belongs in `query` usually belongs in one of those sub-crates.

## Dioxus-specific rules

- **Dioxus 0.7 APIs only.** `cx`, `Scope`, and `use_state` do not exist here — use `use_signal`,
  `use_resource`, `use_effect`, `#[component]`.
- **Never hold a signal read or write across an `.await` point.** `clippy.toml` lists
  `generational_box::GenerationalRef`, `GenerationalRefMut`, and `dioxus_signals::WriteLock` as
  `await-holding-invalid-types` — clippy will fail the build. Drop the borrow before awaiting.
- Prefer owned props (`String`, `Vec<T>`, cloned models) over borrowed props.
- UI state is **not** localized to component files. Global signals live in `ui/src/app_state.rs`
  (`APP_STATE`, theme, UI settings, SQL format settings, toast, etc.), and persistence triggers fire
  from effects there. A UI change often spans both the component and `app_state.rs`.

## Adding a persisted UI setting

Touch all of these together (per `AGENTS.md`):
- `models/src/settings.rs` — defaults + serde-compat tests
- `ui/src/app_state.rs` — matching `set_*` helper
- the settings modal controls
- workspace visibility/filter helpers and any toolbar/toggle entrypoint
- any flow that should auto-open the relevant panel

## Hotspots (edit carefully — these files are large)

- `ui/src/screens/workspace/mod.rs`
- `ui/src/screens/workspace/components/result_table.rs`
- `ui/src/screens/workspace/components/explorer/create_table_modal.rs`
- `query-core/src/lib.rs`
- `acp/src/runtime.rs` and `acp/src/introspection.rs`

## Connection & secrets

- `connection::connect_to_db` is the single entry point for live connections. SQLite connects
  directly; PostgreSQL/MySQL/ClickHouse may use SSH tunnels. Tunnels are registered by session
  identity key and released via `services::release_ssh_tunnel`. Always go through
  `app_state::remove_session` for cleanup so SSH tunnels are released.
- Secrets (connection passwords, DeepSeek/CodeStral API keys) live in the system **keyring**
  (`shovel.connections`), keyed by a hashed `request.identity_key()`. Connection JSON metadata is
  serialized **without** passwords. There is intentionally no plaintext fallback. A failed keyring
  write after metadata succeeds returns a partial-success error — callers may see an error even
  though JSON was written.

## Storage location

All local app data lives under `dirs::data_local_dir()/shovel/` — JSON files for
connections/sessions/queries/history/settings, plus `shovel.db` (chat threads with FTS5,
sqlite-vec semantic cache) and `acp/workspace/...`. Crash logs go to the OS temp dir.

## Embedded ACP agent mode

The `app` binary can run headless as an embedded ACP agent instead of launching the UI:
`shovel acp-agent ollama --model ...` (or `deepseek`). This bypasses `ui::App` entirely.
ACP child spawning retries Unix `ETXTBSY` (os error 26) a few times in `spawn_acp_child`.