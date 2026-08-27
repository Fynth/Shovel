# Driver-session modularity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop matching `DatabaseConnection { Sqlite | Postgres | MySql | ClickHouse }` in `query`, `explorer`, and `acp`; live pools leave `models`; UI talks to `services` via `session_id` and a `Capabilities` snapshot.

**Architecture:** `database::SessionHandle` (`Arc<dyn DriverSession>`) is the live connection. Drivers own catalog SQL, execute, decode, mutations, explain, and ACP introspection. `query` keeps pagination/batch/format and calls `handle.dialect()` plus `QueryExec`. `connection` owns SSH, the builtin factory, and `session_id → SessionHandle`. Temporary `LiveConnection` enum (today's pools) lives in `database` until phase 6 splits it into four driver types.

**Tech Stack:** Rust nightly / edition 2024, sqlx, tokio, Dioxus 0.7, `async-trait` 0.1.89 (already in `acp`, MIT/Apache).

**Spec:** `docs/superpowers/specs/2026-08-27-driver-session-modularity-design.md`

## Global Constraints

- Toolchain is nightly (`rust-toolchain.toml`); crates use `edition = "2024"`.
- CI: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- Dioxus 0.7 only. Never hold a signal read/write across `.await`.
- `ui` may import only `models` and `services`.
- Four backends stay working after every task.
- No plugin host, no data-driven connect forms, no UI crate split.
- Spec invariant `import_csv` implies `row_editing` is **wrong for current product**: ClickHouse already imports CSV and does not edit rows. Keep `import_csv: true` and `row_editing: false` for ClickHouse. Test `import_csv` independently of `mutate().is_some()`.
- `async-trait` is allowed in `database` and `driver-*`. Do not add sqlx to `models` again after Task 7.

---

## File Structure

New files:

- `database/src/capabilities.rs` — re-export only if we keep `Capabilities` in `models` (we do). Skip this file.
- `database/src/dialect.rs` — `Dialect`, `FormatFlavor`, quote helpers.
- `database/src/handle.rs` — `SessionHandle`, `DriverSession`, exec traits.
- `database/src/live.rs` — temporary `LiveConnection` enum (Task 7). Deleted in Task 18.
- `database/src/fake.rs` — `FakeDriver` behind feature `fake`.
- `connection/src/registry.rs` — `register_session` / `unregister_session` / `session`.
- `driver-*/src/session.rs` (Task 18) — per-driver `DriverSession` impl.

Modified (by phase):

- Phase 1: `models/src/connection.rs`, `models/src/app.rs`, `database/src/lib.rs`, `database/Cargo.toml`
- Phase 2: `models` error + session types, `connection`, `services`, `ui` session_id cutover
- Phase 3: `query/src/core/*`, `query/src/format.rs`, `query/src/io.rs`, `query/Cargo.toml`, `driver-*`
- Phase 4: `explorer/src/*`, `acp/src/introspection.rs`, `models` introspection DTOs
- Phase 5: `ui/src/screens/workspace/**` capability checks
- Phase 6: `ARCHITECTURE.md`, delete `LiveConnection`, delete ClickHouse methods on `DatabaseDriver`

---

### Task 1: `Capabilities` in `models`

**Files:**

- Modify: `models/src/connection.rs`
- Modify: `models/src/app.rs` (`ConnectionSession.capabilities` added in Task 6, not here)
- Test: `models/tests/public_api.rs`

**Interfaces:**

- Consumes: `DatabaseKind`
- Produces: `models::Capabilities` with fields `row_editing`, `explain`, `transactions`, `schemas`, `import_csv`, `ssh_tunnel`. Produces `Capabilities::for_kind(DatabaseKind) -> Capabilities`.

- [ ] **Step 1: Write the failing test**

Add to `models/tests/public_api.rs`:

```rust
#[test]
fn capabilities_for_kind_match_current_product() {
    let sqlite = Capabilities::for_kind(DatabaseKind::Sqlite);
    assert!(sqlite.row_editing);
    assert!(sqlite.explain);
    assert!(sqlite.transactions);
    assert!(sqlite.import_csv);
    assert!(!sqlite.ssh_tunnel);
    assert!(!sqlite.schemas);

    let postgres = Capabilities::for_kind(DatabaseKind::Postgres);
    assert!(postgres.row_editing && postgres.explain && postgres.transactions);
    assert!(postgres.schemas && postgres.import_csv && postgres.ssh_tunnel);

    let mysql = Capabilities::for_kind(DatabaseKind::MySql);
    assert!(mysql.row_editing && mysql.explain && mysql.transactions);
    assert!(mysql.schemas && mysql.import_csv && mysql.ssh_tunnel);

    let ch = Capabilities::for_kind(DatabaseKind::ClickHouse);
    assert!(!ch.row_editing);
    assert!(ch.explain);
    assert!(!ch.transactions);
    assert!(ch.schemas);
    assert!(ch.import_csv);
    assert!(ch.ssh_tunnel);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models capabilities_for_kind_match_current_product -- --nocapture`

Expected: FAIL, `Capabilities` not found.

- [ ] **Step 3: Write minimal implementation**

In `models/src/connection.rs` next to `DatabaseKind`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub row_editing: bool,
    pub explain: bool,
    pub transactions: bool,
    pub schemas: bool,
    pub import_csv: bool,
    pub ssh_tunnel: bool,
}

impl Capabilities {
    pub fn for_kind(kind: DatabaseKind) -> Self {
        match kind {
            DatabaseKind::Sqlite => Self {
                row_editing: true,
                explain: true,
                transactions: true,
                schemas: false,
                import_csv: true,
                ssh_tunnel: false,
            },
            DatabaseKind::Postgres | DatabaseKind::MySql => Self {
                row_editing: true,
                explain: true,
                transactions: true,
                schemas: true,
                import_csv: true,
                ssh_tunnel: true,
            },
            DatabaseKind::ClickHouse => Self {
                row_editing: false,
                explain: true,
                transactions: false,
                schemas: true,
                import_csv: true,
                ssh_tunnel: true,
            },
        }
    }
}
```

Leave `DatabaseKind::supports_row_editing` / `supports_ssh_tunnel` in place until Task 17.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models capabilities_for_kind_match_current_product`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/connection.rs models/tests/public_api.rs
git commit -m "feat(models): add Capabilities snapshot for driver sessions"
```

---

### Task 2: `Dialect` in `database`

**Files:**

- Create: `database/src/dialect.rs`
- Modify: `database/src/lib.rs` (mod + pub use)
- Test: `database/src/dialect.rs` (`#[cfg(test)]`)

**Interfaces:**

- Consumes: `models::{QueryFilterOperator, QuerySort}`
- Produces: `database::{Dialect, FormatFlavor}` and helpers `quote_ident_double`, `quote_ident_backtick`.

`Dialect` is `Copy`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatFlavor {
    Postgres,
    Generic,
}

#[derive(Clone, Copy)]
pub struct Dialect {
    pub quote_identifier: fn(&str) -> String,
    pub filter_expression: fn(&str, QueryFilterOperator, &str) -> String,
    pub format_flavor: FormatFlavor,
}
```

Do not delete `query::core::SqlBuildDialect` yet. Task 8 switches query onto this type.

- [ ] **Step 1: Write the failing test**

In `database/src/dialect.rs` (file can exist with only tests first if the types are referenced from tests in the same module; put tests in `database/src/lib.rs` under `#[cfg(test)] mod dialect_tests` if the module is empty).

Prefer creating the module with the helpers and tests together after the failing compile. First add this test module to `database/src/lib.rs`:

```rust
#[cfg(test)]
mod dialect_tests {
    use super::{quote_ident_backtick, quote_ident_double};

    #[test]
    fn double_quote_escapes_inner_quotes() {
        assert_eq!(quote_ident_double("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn backtick_escapes_inner_backticks() {
        assert_eq!(quote_ident_backtick("a`b"), "`a``b`");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p database double_quote_escapes_inner_quotes -- --nocapture`

Expected: FAIL, `quote_ident_double` not found.

- [ ] **Step 3: Write minimal implementation**

`database/src/dialect.rs`:

```rust
use models::QueryFilterOperator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatFlavor {
    Postgres,
    Generic,
}

#[derive(Clone, Copy)]
pub struct Dialect {
    pub quote_identifier: fn(&str) -> String,
    pub filter_expression: fn(&str, QueryFilterOperator, &str) -> String,
    pub format_flavor: FormatFlavor,
}

pub fn quote_ident_double(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub fn quote_ident_backtick(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}
```

`database/src/lib.rs`: `mod dialect; pub use dialect::*;`

Filter function pointers stay in `query` until Task 8/10. `Dialect` can still be constructed in tests with a stub filter:

```rust
fn stub_filter(_col: &str, _op: QueryFilterOperator, _val: &str) -> String {
    "1=1".to_string()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p database`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add database/src/dialect.rs database/src/lib.rs
git commit -m "feat(database): add Dialect and identifier quote helpers"
```

---

### Task 3: Exec traits, `SessionHandle`, `FakeDriver`

**Files:**

- Create: `database/src/handle.rs`
- Create: `database/src/fake.rs`
- Modify: `database/src/lib.rs`
- Modify: `database/Cargo.toml` (feature `fake`, dep `async-trait = "0.1.89"`)

**Interfaces:**

- Consumes: `Capabilities`, `DatabaseKind`, `Dialect`, `QueryOutput`, `QueryFilter`, `QuerySort`, `DatabaseError`, `TablePreviewSource`, `ExplorerNode`, `ExplorerNodeKind`, `TableForeignKey`, `ExecutionPlan`
- Produces:

```rust
#[async_trait]
pub trait QueryExec: Send + Sync {
    /// Run already-built SQL and decode rows. Pagination SQL is built by
    /// `query` via `Dialect`.
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError>;
}

#[async_trait]
pub trait SchemaExec: Send + Sync {
    async fn describe_table(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<QueryOutput, DatabaseError>;
    async fn load_table_columns(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<Vec<String>, DatabaseError>;
    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError>;
    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError>;
    async fn load_object_ddl(
        &self,
        schema: Option<String>,
        object: String,
        kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError>;
}

#[async_trait]
pub trait MutateExec: Send + Sync {
    async fn update_table_cell(
        &self,
        source: TablePreviewSource,
        locator: String,
        column_name: String,
        value: String,
    ) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait ExplainExec: Send + Sync {
    async fn execute_explain(
        &self,
        sql: &str,
        analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError>;
}

#[async_trait]
pub trait IntrospectExec: Send + Sync {
    async fn introspect(&self) -> models::IntrospectionResult;
}
```

`IntrospectionResult` does not exist on `models` yet. For this task, give `IntrospectExec` a placeholder that FakeDriver does not implement (`as_introspect() -> None`). Do not add the DTO until Task 16. Trait:

```rust
pub trait DriverSession: QueryExec + SchemaExec + Send + Sync {
    fn kind(&self) -> DatabaseKind;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> Dialect;
    fn as_mutate(&self) -> Option<&dyn MutateExec>;
    fn as_explain(&self) -> Option<&dyn ExplainExec>;
    fn as_introspect(&self) -> Option<&dyn IntrospectExec>;
}

pub struct SessionHandle {
    inner: Arc<dyn DriverSession>,
}

impl SessionHandle {
    pub fn wrap(inner: Arc<dyn DriverSession>) -> Self;
    pub fn kind(&self) -> DatabaseKind;
    pub fn capabilities(&self) -> Capabilities;
    pub fn dialect(&self) -> Dialect;
    pub fn query(&self) -> &dyn QueryExec;
    pub fn schema(&self) -> &dyn SchemaExec;
    pub fn mutate(&self) -> Option<&dyn MutateExec>;
    pub fn explain(&self) -> Option<&dyn ExplainExec>;
    pub fn introspect(&self) -> Option<&dyn IntrospectExec>;
}
```

`FakeDriver` (feature `fake`): in-memory table `"items"` with columns `id`, `name`. `execute_sql` ignores SQL and returns that table. `row_editing: false`, `as_mutate(): None`. `as_explain(): None`. Schema tree is one table node named `items`.

`database/Cargo.toml`:

```toml
[features]
default = []
fake = []

[dependencies]
async-trait = "0.1.89"
```

- [ ] **Step 1: Write the failing test**

`database/src/handle.rs` `#[cfg(all(test, feature = "fake"))]` will not run by default. Put FakeDriver tests in `database/src/fake.rs` under `#[cfg(test)]` and enable the module with `#[cfg(feature = "fake")]`. Run tests with `--features fake`.

```rust
#[cfg(test)]
mod tests {
    use super::FakeDriver;
    use crate::SessionHandle;
    use std::sync::Arc;

    #[tokio::test]
    async fn fake_execute_sql_returns_rows() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let out = handle.query().execute_sql("select 1").await.unwrap();
        match out {
            models::QueryOutput::Table(page) => {
                assert_eq!(page.columns, vec!["id", "name"]);
                assert!(!page.rows.is_empty());
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn fake_has_no_mutate_when_row_editing_false() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        assert!(!handle.capabilities().row_editing);
        assert!(handle.mutate().is_none());
    }

    #[tokio::test]
    async fn fake_schema_tree_has_items() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let tree = handle.schema().load_connection_tree().await.unwrap();
        assert!(
            tree.iter().any(|node| node.name == "items"),
            "expected items node, got {tree:?}"
        );
    }
}
```

`ExplorerNode` has `name: String` (`models/src/explorer.rs`). Fake tree node must set `name: "items"`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p database --features fake -- --nocapture`

Expected: FAIL (module/types missing).

- [ ] **Step 3: Implement traits, handle, FakeDriver**

`DriverSession` methods `query()`/`schema()` on the handle return `self.inner.as_ref()` coerced to `&dyn QueryExec` / `&dyn SchemaExec`.

For FakeDriver `execute_sql`, return `QueryOutput::Table(QueryPage { columns: vec!["id".into(), "name".into()], rows: vec![vec!["1".into(), "alpha".into()]], editable: None, offset: 0, page_size: 10, has_previous: false, has_next: false })`.

Stub remaining `SchemaExec` methods: `describe_table` same page, `load_table_columns` returns `id,name`, `load_foreign_keys` empty, `load_object_ddl` `None`.

`IntrospectExec` can be an empty trait with `async fn introspect(&self) {}` returning `()` until Task 16, or omit the trait body usage. Define:

```rust
#[async_trait]
pub trait IntrospectExec: Send + Sync {}
```

Empty traits with async_trait are useless. Skip `IntrospectExec` methods until Task 16; keep `as_introspect() -> Option<&dyn IntrospectExec>` with the trait having one method:

```rust
#[async_trait]
pub trait IntrospectExec: Send + Sync {
    async fn ping(&self) -> Result<(), DatabaseError>;
}
```

Task 16 replaces `ping` with `introspect`. Fake returns `None` for introspect, so `ping` is never called.

- [ ] **Step 4: Run tests**

Run: `cargo test -p database --features fake`

Expected: PASS.

Also: `cargo test -p database` (no feature) still PASS (fake module not compiled).

- [ ] **Step 5: Commit**

```bash
git add database/Cargo.toml database/src/lib.rs database/src/handle.rs database/src/fake.rs
git commit -m "feat(database): SessionHandle, exec traits, and FakeDriver"
```

---

### Task 4: Flatten `DatabaseError`

**Files:**

- Modify: `models/src/connection.rs` (`DatabaseError` enum, `Display`, drop `kind()`)
- Modify: `models/tests/public_api.rs`
- Modify every `DatabaseError::Sqlite(` / `Postgres(` / `MySql(` / `ClickHouse(` / `UnsupportedDriver(` site. Find with:

```bash
rg -n "DatabaseError::(Sqlite|Postgres|MySql|ClickHouse|UnsupportedDriver)" --type rust
```

**Interfaces:**

- Produces:

```rust
pub enum DatabaseError {
    Driver(String),
    Tunnel(String),
    Unsupported(String),
    SessionNotFound(u64),
}
```

Display:

- `Driver`: `"{err}"` (no sqlite/postgres prefix; UI does not branch on backend)
- `Tunnel`: `"SSH tunnel error: {err}"`
- `Unsupported`: `"{err}"`
- `SessionNotFound`: `"session {id} is not connected"`

Replace:

- `DatabaseError::Sqlite(e)` / `Postgres(e)` / `MySql(e)` → `DatabaseError::Driver(e.to_string())`
- `DatabaseError::ClickHouse(s)` → `DatabaseError::Driver(s)`
- `DatabaseError::UnsupportedDriver(s)` → `DatabaseError::Unsupported(s)`

`database/src/lib.rs` default methods on `DatabaseDriver` that return `UnsupportedDriver` must use `Unsupported`.

Do not remove sqlx from `models` yet (`DatabaseConnection` still holds pools).

- [ ] **Step 1: Write the failing test**

Replace `database_error_kind_reports_origin` and `database_error_displays_with_kind_prefix` in `models/tests/public_api.rs`:

```rust
#[test]
fn database_error_display_is_unprefixed_driver_string() {
    let err = DatabaseError::Driver("bad request".into());
    assert_eq!(err.to_string(), "bad request");
}

#[test]
fn session_not_found_display_includes_id() {
    let err = DatabaseError::SessionNotFound(7);
    assert!(err.to_string().contains("7"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models database_error_display_is_unprefixed_driver_string`

Expected: FAIL, `Driver` variant missing.

- [ ] **Step 3: Change the enum and fix the workspace**

Update `Display` and delete `kind()`. Then run `cargo test --workspace`. Fix every compile error from the rg list. ClickHouse driver maps HTTP errors with `DatabaseError::Driver(...)`.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`

Expected: PASS. If a test asserted `err.kind() == Some(ClickHouse)`, rewrite it to match `Driver`.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "refactor(models): flatten DatabaseError to Driver/Tunnel/Unsupported/SessionNotFound"
```

---

### Task 5: Session registry and `connect_to_db` → `SessionHandle`

**Files:**

- Create: `connection/src/registry.rs`
- Modify: `connection/src/lib.rs`
- Modify: `database/src/handle.rs` (`SessionHandle::from_live` temporary)
- Test: `connection/src/registry.rs` `#[cfg(test)]` using FakeDriver (`connection` dev-dep `database` with `features = ["fake"]`)

**Interfaces:**

- Consumes: `SessionHandle`
- Produces:

```rust
pub fn register_session(id: u64, handle: SessionHandle);
pub fn unregister_session(id: u64) -> Option<SessionHandle>;
pub fn session(id: u64) -> Option<SessionHandle>;
```

Registry: `std::sync::LazyLock<std::sync::RwLock<HashMap<u64, SessionHandle>>>`.

Temporary constructor, still using `models::DatabaseConnection`:

```rust
impl SessionHandle {
    pub fn from_legacy(connection: models::DatabaseConnection) -> Self;
    pub fn legacy(&self) -> Option<models::DatabaseConnection>;
}
```

`from_legacy` wraps a private `LegacyDriver` in `database/src/handle.rs` that:

- `kind()` / `capabilities()` / `dialect()` from the enum variant (dialect filter fns can be stubs that panic if called; query still matches `legacy()` in this task)
- `QueryExec`/`SchemaExec` return `DatabaseError::Unsupported("legacy driver; use SessionHandle::legacy")` if called
- `as_mutate`/`as_explain`/`as_introspect` follow `Capabilities::for_kind` (`Some` dummy that returns `Unsupported` is worse). Return `None` for mutate if `!row_editing`, else `Some` dummy. Prefer: store the `DatabaseConnection` and implement `legacy()` only. QueryExec can still return Unsupported. Production query path in this task still uses `handle.legacy()`.

`connect_to_db` return type becomes `Result<SessionHandle, DatabaseError>`. Body: existing match, then `Ok(SessionHandle::from_legacy(DatabaseConnection::...))`.

Do not change UI in this task. `services::connect_and_save_request` still compiles if `ConnectAndSaveResult.connection` type updates to `SessionHandle` — that forces UI compile errors. **Keep `ConnectAndSaveResult` as `DatabaseConnection` in this task** by mapping:

```rust
let handle = connection::connect_to_db(request.clone()).await?;
let save_warning = storage::save_connection_request(request).await.err();
Ok(ConnectAndSaveResult {
    connection: handle.legacy().expect("legacy handle"),
    save_warning,
})
```

That is ugly but keeps the workspace green. Task 6 switches the result type.

Alternatively change `connect_to_db` here and immediately fix `services` to unwrap `.legacy()`. Yes: services still returns `DatabaseConnection` via `.legacy()`.

- [ ] **Step 1: Write the failing test**

`connection/src/registry.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{register_session, session, unregister_session};
    use database::{FakeDriver, SessionHandle};
    use std::sync::Arc;

    #[test]
    fn register_get_unregister() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        register_session(42, handle.clone());
        assert!(session(42).is_some());
        assert!(unregister_session(42).is_some());
        assert!(session(42).is_none());
    }
}
```

`connection/Cargo.toml` dev-dependencies:

```toml
[dev-dependencies]
database = { workspace = true, features = ["fake"] }
```

Production `database` dep stays without `fake`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p connection register_get_unregister -- --nocapture`

Expected: FAIL, `register_session` missing.

- [ ] **Step 3: Implement registry, `from_legacy`, change `connect_to_db`**

`LegacyDriver` must be `Clone` via cloned sqlx pools (they are internally Arc). `DatabaseConnection` is already `Clone`.

`session()` returns `handle.clone()` (`SessionHandle` is `Clone` via `Arc`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p connection` and `cargo test --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add connection/src/registry.rs connection/src/lib.rs connection/Cargo.toml database/src/handle.rs services/src/app.rs
git commit -m "feat(connection): session registry and SessionHandle from connect_to_db"
```

---

### Task 6: UI talks `session_id`; `ConnectionSession` drops the pool

**Files:**

- Modify: `models/src/app.rs`
- Modify: `ui/src/app_state/mod.rs` (`add_connection_session`, `remove_session`, `session_connection`, `restore_connection_sessions`)
- Modify: `services/src/app.rs` (`ConnectAndSaveResult`, `SessionRestoreResult`)
- Modify: `services/src/lib.rs` (re-export `register_session` if UI needs it — **no**: UI must not import `connection`. `add_connection_session` in `app_state` calls `services::register_session`)
- Modify: every UI caller of `session_connection` / `.connection` / `ConnectAndSaveResult.connection` (rg list from exploration: `ui/src/app.rs`, `recent_connections.rs`, `workspace/actions.rs`, `helpers.rs`, `windows/mod.rs`, `table_structure.rs`, `agent_panel/*`, `explorer/*`, `tabs.rs`, `use_acp.rs`)

**Interfaces:**

```rust
pub struct ConnectionSession {
    pub id: u64,
    pub name: String,
    pub kind: DatabaseKind,
    pub request: ConnectionRequest,
    pub capabilities: Capabilities,
}

pub struct ConnectAndSaveResult {
    pub handle: SessionHandle, // re-exported from services; UI must not store it
}
```

`SessionHandle` must not appear in `models` or in UI props. `ConnectAndSaveResult` lives in `services` and may hold `SessionHandle` because `services` depends on `connection`/`database`. UI code:

```rust
let saved = services::connect_and_save_request(request.clone()).await?;
let id = crate::app_state::add_connection_session(request, saved.handle);
```

`add_connection_session(request, handle) -> u64`:

1. Allocate or reuse `session_id` as today (identity_key match).
2. If replacing, `services::unregister_session(old_id)`.
3. `services::register_session(id, handle)`.
4. Push `ConnectionSession { id, name, kind, request, capabilities: handle.capabilities() }`.

`remove_session`: `unregister_session` then `release_ssh_tunnel` as today.

Replace `session_connection(id) -> Option<DatabaseConnection>` with nothing in UI. Call sites that did `services::execute_query_page(connection, ...)` become `services::execute_query_page(session_id, ...)`.

`services` re-exports `SessionHandle` for the connect handshake only. `ConnectionSession` never stores it. Until Task 9, `query` still takes `LiveConnection` internally. `services::execute_query_page(session_id, ...)` looks up the handle and uses `.legacy()`.

Until query is migrated (Task 9), services does:

```rust
pub async fn execute_query_page(
    session_id: u64,
    sql: String,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let handle = connection::session(session_id)
        .ok_or(DatabaseError::SessionNotFound(session_id))?;
    let live = handle
        .legacy()
        .ok_or_else(|| DatabaseError::Unsupported("session has no live connection".into()))?;
    query::execute_query_page(live, sql, page_size, offset, filter, sort).await
}
```

Windows: `CreateTableWindowRootProps.connection: Option<DatabaseConnection>` → `session_id: Option<u64>`. PartialEq compares the id.

Agent panel `state.connection: Option<DatabaseConnection>` → `session_id: Option<u64>`.

- [ ] **Step 1: Write the failing test**

Add `services/tests/session_lookup.rs`:

```rust
#[tokio::test]
async fn execute_unknown_session_is_session_not_found() {
    let err = services::execute_query_page(999_999, "select 1".into(), 10, 0, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, models::DatabaseError::SessionNotFound(999_999)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p services execute_unknown_session_is_session_not_found -- --nocapture`

Expected: FAIL (arity of `execute_query_page` still takes `DatabaseConnection`).

- [ ] **Step 3: Change services signatures, then UI**

Update `services/src/lib.rs` re-exports: the `pub use query::execute_query_page` will clash with a new wrapper. Stop re-exporting query's function of the same name. Define wrappers in `services/src/app.rs` or new `services/src/session_ops.rs` and export those.

Keep `query::execute_query_page` (legacy connection) `pub(crate)` or public for driver tests until Task 8.

Update `services/tests/facade_smoke.rs`: the symbol `execute_query_page` still exists, now with `session_id`.

Grep UI for `session_connection(` and `saved.connection` and fix each site to `session_id`.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/app.rs services ui
git commit -m "feat(ui): persist capabilities and route DB ops through session_id"
```

---

### Task 7: Remove sqlx from `models`

**Files:**

- Create: `database/src/live.rs` (`LiveConnection` = today's `DatabaseConnection`)
- Modify: `models/src/connection.rs` (delete enum `DatabaseConnection` and `is_sqlite` etc.)
- Modify: `models/Cargo.toml` (drop sqlx)
- Modify: `database/Cargo.toml` (add sqlx with sqlite/postgres/mysql)
- Modify: `query`, `explorer`, `connection`, `acp`, `database` `from_legacy` to use `database::LiveConnection`

**Interfaces:**

```rust
pub enum LiveConnection {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
    MySql(sqlx::MySqlPool),
    ClickHouse(models::ClickHouseFormData),
}
```

`SessionHandle::legacy() -> Option<LiveConnection>`.

`models` must compile without sqlx. `models/tests/public_api.rs` must not mention `DatabaseConnection`.

- [ ] **Step 1: Write the failing test**

Add a compile-oriented unit test in `models/tests/public_api.rs` that `ConnectionSession` has `capabilities` and no `connection` field (already true after Task 6). Add:

```rust
#[test]
fn models_connection_session_has_no_live_pool_field() {
    let session = models::ConnectionSession {
        id: 1,
        name: "s".into(),
        kind: DatabaseKind::Sqlite,
        request: sqlite_request(),
        capabilities: Capabilities::for_kind(DatabaseKind::Sqlite),
    };
    assert_eq!(session.capabilities.row_editing, true);
}
```

The real gate is `cargo tree -p models -i sqlx` after the Cargo.toml edit.

- [ ] **Step 2: Run test to verify it fails**

If Task 6 already added `capabilities`, this test may pass. The failing step is removing sqlx: `cargo check -p models` after deleting the dep while `DatabaseConnection` still exists. Do the deletion in Step 3.

- [ ] **Step 3: Move the enum, drop sqlx from models**

Move `DatabaseConnection` body to `database::LiveConnection`. Replace every remaining `models::DatabaseConnection` (should be none in UI). `rg "DatabaseConnection"`.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`

Run: `cargo tree -p models -i sqlx`

Expected: tests PASS; `cargo tree` reports sqlx is not a models dependency.

- [ ] **Step 5: Commit**

```bash
git add models database query explorer connection acp
git commit -m "refactor: move live pools out of models into database::LiveConnection"
```

---

### Task 8: Query pagination uses `Dialect`; `format_sql` uses `FormatFlavor`

**Files:**

- Modify: `query/src/core/build.rs` (`SqlBuildDialect` → `database::Dialect`)
- Modify: `query/src/core/mod.rs` (constants `SQLITE_DIALECT` etc. become `database::Dialect { format_flavor, ... }`)
- Modify: `query/src/format.rs`
- Modify: `query/examples/format_sql.rs` and tests that pass `Option<DatabaseKind>`

**Interfaces:**

```rust
pub fn format_sql(flavor: FormatFlavor, sql: &str, settings: &SqlFormatSettings) -> String;
```

Map `FormatFlavor::Postgres` → `sqlformat::Dialect::PostgreSql`, `Generic` → `Generic`.

`execute_query_page` still takes `LiveConnection` internally this task; only dialect construction changes. `SQLITE_DIALECT.filter_expression` remains the existing sqlite/postgres/mysql/clickhouse functions in `build.rs`, now assigned into `database::Dialect`.

- [ ] **Step 1: Write the failing test**

In `query/src/format.rs` tests (or `query/src/lib.rs` tests):

```rust
#[test]
fn format_sql_uses_format_flavor_not_database_kind() {
    let settings = SqlFormatSettings::default();
    let out = format_sql(FormatFlavor::Postgres, "select 1", &settings);
    assert!(!out.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p query format_sql_uses_format_flavor_not_database_kind -- --nocapture`

Expected: FAIL (signature still `Option<DatabaseKind>`).

- [ ] **Step 3: Change `format_sql` and `SqlBuildDialect`**

Update all callers (`rg format_sql`). UI should call `services::format_sql` with `session.capabilities` is wrong; use `handle.dialect().format_flavor`. After Task 6 UI has no handle. Add `services::format_sql_for_session(session_id, sql, settings)` that looks up handle dialect, or pass `FormatFlavor` from `Capabilities` (not there). Lookup is correct:

```rust
pub fn format_sql_for_session(
    session_id: u64,
    sql: &str,
    settings: &SqlFormatSettings,
) -> Result<String, DatabaseError> {
    let handle = connection::session(session_id)
        .ok_or(DatabaseError::SessionNotFound(session_id))?;
    Ok(query::format_sql(handle.dialect().format_flavor, sql, settings))
}
```

UI format button uses this.

- [ ] **Step 4: Run tests**

Run: `cargo test -p query` and `cargo test --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add query services ui
git commit -m "refactor(query): drive SQL format and pagination dialect without DatabaseKind"
```

---

### Task 9: `query::execute_query_page` takes `&SessionHandle`

**Files:**

- Modify: `query/src/core/mod.rs`
- Modify: `query/src/core/preview.rs`, `mutations.rs`, `ddl.rs`, `execution_plan.rs`, `query/src/io.rs`
- Modify: `services/src/session_ops.rs` (or `app.rs`) to stop using `.legacy()` for the happy path

**Interfaces:**

Every query entrypoint listed in `services/src/lib.rs` that took `DatabaseConnection` / `LiveConnection` now takes `&SessionHandle`.

Temporary body:

```rust
pub async fn execute_query_page(
    handle: &SessionHandle,
    sql: String,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let dialect = handle.dialect();
    let built_sql = build_paginated_query(
        &sql,
        page_size,
        offset,
        filter.as_ref(),
        sort.as_ref(),
        dialect,
    );
    match handle.query().execute_sql(&built_sql).await {
        Ok(out) => Ok(out),
        Err(DatabaseError::Unsupported(_)) => {
            let live = handle.legacy().ok_or(DatabaseError::Unsupported(
                "no query exec and no live connection".into(),
            ))?;
            execute_query_page_live(live, sql, page_size, offset, filter, sort).await
        }
        Err(err) => Err(err),
    }
}
```

Rename today's function to `execute_query_page_live`. FakeDriver then works through query. Real drivers still hit the live fallback.

- [ ] **Step 1: Write the failing test**

`query/src/core/mod.rs` or `query/tests/fake_handle.rs` with `database` dev-feature `fake`:

```rust
#[tokio::test]
async fn execute_query_page_uses_fake_query_exec() {
    let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
    let out = execute_query_page(&handle, "select 1".into(), 10, 0, None, None)
        .await
        .unwrap();
    assert!(matches!(out, QueryOutput::Table(_)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p query execute_query_page_uses_fake_query_exec -- --nocapture`

Expected: FAIL (still wants `LiveConnection`).

- [ ] **Step 3: Implement the handle signature and fallback**

Wire services wrappers to pass `&handle` after registry lookup.

- [ ] **Step 4: Run tests**

Run: `cargo test -p query` and `cargo test --workspace`

Expected: PASS, including FakeDriver test.

- [ ] **Step 5: Commit**

```bash
git add query services
git commit -m "refactor(query): execute and mutate against SessionHandle"
```

---

### Task 10: Move SQLite execute/decode into `driver-sqlite`

**Files:**

- Modify: `driver-sqlite/src/lib.rs` (or `driver-sqlite/src/query.rs`)
- Modify: `query/src/core/mod.rs`, `rows.rs` (remove sqlite match arm / sqlite row helpers used only here)
- Modify: `database/src/handle.rs` `LegacyDriver` for Sqlite variant: construct `SqliteSession` instead of fallback

**Interfaces:**

`SqliteSession` implements `DriverSession + QueryExec + SchemaExec` (schema still stub until Task 14). `QueryExec::execute_sql` is today's sqlite fetch + row decode.

`connect_to_db` for sqlite: `SessionHandle::wrap(Arc::new(SqliteSession { pool }))` instead of `from_legacy`.

Keep `LiveConnection::Sqlite` for remaining query functions (preview/mutations) until those move; `legacy()` on `SqliteSession` returns `Some(LiveConnection::Sqlite(pool.clone()))` so fallback still works for mutations.

- [ ] **Step 1: Write the failing test**

`driver-sqlite/src/lib.rs`:

```rust
#[tokio::test]
async fn sqlite_session_executes_select() {
    let pool = SqliteDriver::connect(":memory:".into()).await.unwrap();
    sqlx::query("create table items (id integer, name text)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("insert into items values (1, 'a')")
        .execute(&pool)
        .await
        .unwrap();
    let handle = SessionHandle::wrap(Arc::new(SqliteSession { pool }));
    let out = handle
        .query()
        .execute_sql("select id, name from items")
        .await
        .unwrap();
    match out {
        QueryOutput::Table(page) => assert_eq!(page.rows.len(), 1),
        other => panic!("{other:?}"),
    }
}
```

`SqliteSession` is the type this task adds. Test fails until it exists.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p driver-sqlite sqlite_session_executes_select -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Move sqlite execute + decode**

`query` builds the paginated string via `handle.dialect()` (including editable locator). `SqliteSession::execute_sql` only fetches and decodes. Do not call `query` from `driver-sqlite` (cycle).

Add deps on `driver-sqlite`: `models`, `async-trait`. Move `sqlite_*_paginated_page` helpers from `query/src/core/rows.rs`.

If preview vs plain page need different decode, add to `database`:

```rust
pub enum DecodeMode {
    Page { page_size: u32, offset: u64 },
    Preview {
        source: TablePreviewSource,
        page_size: u32,
        offset: u64,
    },
}
```

and extend `QueryExec` with `execute_sql_decoded(&self, sql: &str, mode: DecodeMode)` **only if** a single `execute_sql` cannot infer preview from the `__shovel_locator` column already in the result. Prefer inferring from the column so the trait stays one method.

- [ ] **Step 4: Run tests**

Run: `cargo test -p driver-sqlite` and `cargo test -p query` and `cargo test --workspace`

Expected: PASS. `execute_query_page` sqlite arm is gone; sqlite goes through `SqliteSession::execute_sql`.

- [ ] **Step 5: Commit**

```bash
git add driver-sqlite query database connection
git commit -m "refactor(driver-sqlite): own query execute and row decode"
```

---

### Task 11: Move PostgreSQL execute/decode into `driver-postgres`

Same shape as Task 10 for Postgres. Test uses a real server only if CI has one; if not, unit-test decode helpers with fixture rows **or** skip live connect and test that `PostgresSession` implements the trait via a compile test plus moving the existing query tests.

If `query` has no live postgres CI, do not add one. Move code, keep `cargo test --workspace` green.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn postgres_session_is_a_driver_session() {
    fn assert_session<T: database::DriverSession>() {}
    assert_session::<driver_postgres::PostgresSession>();
}
```

`PostgresSession` needs to exist. This fails first.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p driver-postgres postgres_session_is_a_driver_session -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Move postgres execute + decode; `connect_to_db` wraps `PostgresSession`**

- [ ] **Step 4: Run `cargo test --workspace`**

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add driver-postgres query connection
git commit -m "refactor(driver-postgres): own query execute and row decode"
```

---

### Task 12: Move MySQL execute/decode into `driver-mysql`

**Files:**

- Modify: `driver-mysql/src/lib.rs` (add `MysqlSession`)
- Modify: `query/src/core/mod.rs`, `rows.rs` (remove MySQL match arm / mysql row helpers used only here)
- Modify: `connection/src/lib.rs` (`connect_to_db` wraps `MysqlSession`)

**Interfaces:**

`MysqlSession { pool: sqlx::MySqlPool }` implements `DriverSession + QueryExec`. `execute_sql` is today's mysql fetch + row decode. `legacy()` returns `Some(LiveConnection::MySql(pool.clone()))` until mutations move in Task 13.

```rust
pub struct MysqlSession {
    pub pool: sqlx::MySqlPool,
}
```

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn mysql_session_is_a_driver_session() {
    fn assert_session<T: database::DriverSession>() {}
    assert_session::<driver_mysql::MysqlSession>();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p driver-mysql mysql_session_is_a_driver_session -- --nocapture`

Expected: FAIL, `MysqlSession` not found.

- [ ] **Step 3: Move mysql execute + decode; `connect_to_db` wraps `MysqlSession`**

Move `execute_mysql_query_page` and `mysql_*_paginated_page` from `query` into `driver-mysql`. Add deps `models`, `async-trait`. `query` must not depend on `driver-mysql`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p driver-mysql` and `cargo test --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add driver-mysql query connection
git commit -m "refactor(driver-mysql): own query execute and row decode"
```

---

### Task 13: Move ClickHouse execute/decode; drop sqlx from `query`

**Files:**

- Modify: `driver-clickhouse/src/lib.rs`
- Modify: `query/src/core/mod.rs`, `rows.rs`, `io.rs`, `query/Cargo.toml`
- Delete ClickHouse JSON methods from `database::DatabaseDriver` **only if unused**. They are still used until this task. After move, `ClickHouseSession::execute_sql` calls the HTTP client internally. Then remove `execute_json_query` / `execute_text_query` from the shared `DatabaseDriver` trait.

**Interfaces:**

`ClickHouseSession { config: ClickHouseFormData }` implements `QueryExec`. `as_mutate()` is `None`. `import_csv` stays true; `query/src/io.rs` import for ClickHouse calls `execute_sql` on the handle, not MutateExec.

Remove `query` deps: `sqlx`, `driver-clickhouse`. `query` depends on `database` + `models` only.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn clickhouse_session_has_no_mutate() {
    let session = ClickHouseSession {
        config: models::ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        },
    };
    let handle = SessionHandle::wrap(Arc::new(session));
    assert!(handle.mutate().is_none());
    assert!(!handle.capabilities().row_editing);
    assert!(handle.capabilities().import_csv);
}
```

- [ ] **Step 2: Run to see FAIL**

Run: `cargo test -p driver-clickhouse clickhouse_session_has_no_mutate -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Move ClickHouse execute/decode/import HTTP; delete query match arms; drop sqlx from query**

Also move mutations/ddl/explain/preview sqlite+pg+mysql that still use `.legacy()` in this task if they still match `LiveConnection`. If any `LiveConnection` match remains in `query`, this task is not done. Mutations must already live on `MutateExec` implemented by sqlite/postgres/mysql sessions (move them here if Tasks 10–12 only moved SELECT).

Checklist before leaving the task: `rg "LiveConnection::" query` is empty. `rg "sqlx" query/Cargo.toml` is empty.

- [ ] **Step 4: Run tests**

Run: `cargo test -p query` and `cargo test --workspace`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add driver-clickhouse query database
git commit -m "refactor(query): drop sqlx; ClickHouse execute lives in driver-clickhouse"
```

---

### Task 14: `SchemaExec` on SQLite (move `explorer/src/sqlite.rs`)

**Files:**

- Modify: `driver-sqlite` (add schema methods)
- Modify: `explorer/src/lib.rs`, delete sqlite match internals

**Interfaces:**

`explorer::load_connection_tree(handle: &SessionHandle)` → `handle.schema().load_connection_tree().await`.

Move `load_connection_tree_sqlite`, `describe_table_sqlite`, `load_table_columns_sqlite`, `load_foreign_keys_sqlite`, `load_object_ddl_sqlite` into `driver-sqlite`.

- [ ] **Step 1: Write the failing test**

Extend the `:memory:` test from Task 10:

```rust
#[tokio::test]
async fn sqlite_session_lists_created_table() {
    let pool = SqliteDriver::connect(":memory:".into()).await.unwrap();
    sqlx::query("create table items (id integer)").execute(&pool).await.unwrap();
    let handle = SessionHandle::wrap(Arc::new(SqliteSession { pool }));
    let tree = handle.schema().load_connection_tree().await.unwrap();
    assert!(
        tree.iter().any(|n| n.name.contains("items")),
        "expected items in {tree:?}"
    );
}
```

Use the real `ExplorerNode` field names.

- [ ] **Step 2: Run to see FAIL** (schema still stub/Unsupported)

- [ ] **Step 3: Move sqlite explorer functions; explorer sqlite arm calls handle only**

`explorer` still matches other backends this task.

- [ ] **Step 4: `cargo test --workspace` PASS**

- [ ] **Step 5: Commit `refactor(driver-sqlite): own schema explorer SQL`**

---

### Task 15: SchemaExec on Postgres, MySQL, ClickHouse; explorer becomes a proxy

**Files:**

- Modify: `driver-postgres`, `driver-mysql`, `driver-clickhouse`
- Modify: `explorer/src/lib.rs` (no `match`)
- Delete: `explorer/src/sqlite.rs`, `postgres.rs`, `mysql.rs` after moves
- Modify: `explorer/Cargo.toml` (drop sqlx and `driver-clickhouse`)

**Interfaces:**

```rust
pub async fn load_connection_tree(
    handle: &SessionHandle,
) -> Result<Vec<ExplorerNode>, DatabaseError> {
    handle.schema().load_connection_tree().await
}
```

Same one-liner for `describe_table`, `load_table_columns`, `load_foreign_keys`, `load_object_ddl`.

ClickHouse FK stays `Ok(vec![])` inside the driver.

- [ ] **Step 1: Write the failing test**

`explorer/tests/proxy.rs`:

```rust
#[tokio::test]
async fn explorer_proxy_uses_fake_schema() {
    let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
    let tree = explorer::load_connection_tree(&handle).await.unwrap();
    assert!(!tree.is_empty());
}
```

Enable `database` feature `fake` on explorer dev-deps.

- [ ] **Step 2: Run to see FAIL** (explorer still takes `LiveConnection`)

- [ ] **Step 3: Move remaining explorer SQL; delete per-db modules; drop sqlx from explorer**

- [ ] **Step 4: `cargo test --workspace` PASS.** `rg "LiveConnection" explorer` empty.

- [ ] **Step 5: Commit `refactor(explorer): proxy SchemaExec and drop sqlx`**

---

### Task 16: `IntrospectExec`; move ACP introspection SQL into drivers

**Files:**

- Modify: `models` (move DTO structs `IntrospectionResult`, `LockInfo`, `ActiveQueryInfo`, `QueryHistoryEntry`, `IndexStat`, `TableStat`, `SchemaInfo` from `acp/src/introspection.rs` if they must be on the trait). If moving is too large, keep DTOs in `acp` and define `IntrospectExec` in `acp` — **forbidden by spec** (traits live in `database`). Move DTOs to `models/src/introspection.rs`.
- Modify: `database/src/handle.rs` (`IntrospectExec::introspect`)
- Modify: each `driver-*`
- Modify: `acp/src/introspection.rs` to call `handle.introspect()`
- Modify: `acp/Cargo.toml` (drop sqlx and `driver-clickhouse` if unused)

**Interfaces:**

```rust
#[async_trait]
pub trait IntrospectExec: Send + Sync {
    async fn introspect(&self) -> models::IntrospectionResult;
}
```

All four drivers return `Some`. FakeDriver stays `None` (ACP context without locks).

Dedicated pool: `IntrospectionPool::from_request` calls `connection::connect_to_db` and **does not** `register_session`. Drop the handle when the pool drops. Do not share the UI session's handle (keeps max-2-connections behavior).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn introspection_result_default_is_empty() {
    let result = models::IntrospectionResult::default();
    assert!(result.locks.is_empty());
}
```

Then a FakeDriver test: `handle.introspect().is_none()`.

- [ ] **Step 2: Run to see FAIL** (`models::IntrospectionResult` missing)

- [ ] **Step 3: Move DTOs, implement introspect on four drivers, delete match in acp**

- [ ] **Step 4: `cargo test --workspace` PASS.** `rg "LiveConnection" acp` empty. `rg "DatabaseConnection::" query explorer acp` empty.

- [ ] **Step 5: Commit `refactor(acp): IntrospectExec on drivers, drop sqlx from acp`**

---

### Task 17: Workspace UI uses `Capabilities`

**Files:**

- Modify: `ui/src/screens/workspace/**` wherever behavior keys off `DatabaseKind` / `is_clickhouse` / `supports_row_editing`
- Modify: `models/src/connection.rs` (delete `supports_row_editing`, `supports_ssh_tunnel`, `is_sqlite` and siblings if still present)
- Keep connect-screen `match DatabaseKind` for the four forms

**Interfaces:**

- Row editor, cell edit, import CSV, explain: `session.capabilities.row_editing` / `import_csv` / `explain`
- Labels: `session.kind.display_name()` (already exists). Delete helper matches that only map kind → "PostgreSQL"

`rg "DatabaseKind::" ui/src/screens/workspace` after this task: only allowed for display names if `display_name()` cannot be used, and connect-unrelated identifier quoting must go through services (if a modal still builds dialect SQL, call a services helper instead of matching kind).

- [ ] **Step 1: Write the failing test**

If there is no UI unit test harness for capabilities, add a pure helper in `ui/src/screens/workspace/helpers.rs`:

```rust
pub fn can_edit_rows(capabilities: Capabilities) -> bool {
    capabilities.row_editing
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{Capabilities, DatabaseKind};

    #[test]
    fn clickhouse_cannot_edit_rows() {
        assert!(!can_edit_rows(Capabilities::for_kind(DatabaseKind::ClickHouse)));
    }

    #[test]
    fn sqlite_can_edit_rows() {
        assert!(can_edit_rows(Capabilities::for_kind(DatabaseKind::Sqlite)));
    }
}
```

Wire the table editor / import / explain buttons to `can_edit_rows(session.capabilities)` (and the other flags). Test fails until the helper exists.

- [ ] **Step 2: Run `cargo test -p ui clickhouse_cannot_edit_rows` — FAIL**

- [ ] **Step 3: Add helper, switch workspace buttons, delete `supports_*` from `DatabaseKind`**

Connect forms still hardcoded. SQLite form has no SSH fields regardless of capabilities.

- [ ] **Step 4: `cargo test --workspace` PASS**

- [ ] **Step 5: Commit `feat(ui): gate workspace actions on Capabilities`**

---

### Task 18: Delete `LiveConnection` and `SessionHandle::legacy`

**Files:**

- Delete: `database/src/live.rs`
- Modify: `database/src/handle.rs` (remove `from_legacy`, `legacy`, `LegacyDriver`)
- Modify: `connection/src/lib.rs` (factory only constructs the four `*Session` types)
- Modify: `database/Cargo.toml` (drop sqlx if nothing in `database` needs it)

**Interfaces:**

`connect_to_db` match on `ConnectionRequest` remains the single builtin registry:

```rust
ConnectionRequest::Sqlite(data) => {
    let pool = SqliteDriver::connect(data.path).await.map_err(|e| DatabaseError::Driver(e.to_string()))?;
    Ok(SessionHandle::wrap(Arc::new(SqliteSession { pool })))
}
```

`rg "LiveConnection" --type rust` must be empty.
`rg "SessionHandle::legacy" --type rust` must be empty.

Capability invariants (without a live DB):

```rust
#[test]
fn sqlite_capabilities_match_exec_options() {
    // Construct SqliteSession with a dummy pool only if cheap; otherwise
    // test FakeDriver + ClickHouseSession (no network).
}
```

ClickHouseSession (no network) + FakeDriver cover `row_editing == mutate().is_some()` and `explain == explain().is_some()`. For sqlite/postgres/mysql, add the same assert in each driver's unit test next to `*_session_is_a_driver_session`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn clickhouse_capabilities_match_exec_options() {
    let handle = SessionHandle::wrap(Arc::new(ClickHouseSession { config: /* ... */ }));
    assert_eq!(handle.capabilities().row_editing, handle.mutate().is_some());
    assert_eq!(handle.capabilities().explain, handle.explain().is_some());
}
```

If `legacy` still exists, add a compile-fail comment in this task's Step 3 deletion rather than a test.

- [ ] **Step 2: Run to see FAIL or already pass; then delete `live.rs` and fix compiles**

- [ ] **Step 3: Delete `LiveConnection`, sqlx from `database` if unused, `DatabaseDriver` ClickHouse default methods**

Keep `DatabaseDriver::connect` as the low-level pool factory used by `*Session` construction.

- [ ] **Step 4: Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`**

Expected: PASS. `rg "DatabaseConnection::" query explorer acp` empty. `models/Cargo.toml` has no sqlx.

- [ ] **Step 5: Commit `refactor(database): remove LiveConnection; drivers are the session impls`**

---

### Task 19: `ARCHITECTURE.md` and readiness grep

**Files:**

- Modify: `ARCHITECTURE.md` (layer diagram, `DatabaseDriver` description, persistence of live connections, `services` session_id API)
- Modify: `AGENTS.md` only if it still says UI may hold pools (keep it accurate)

**Interfaces:** none.

- [ ] **Step 1: Write the failing check as a shell assertion in the task log**

Run:

```bash
rg "DatabaseConnection::" query explorer acp
rg "sqlx" models/Cargo.toml
rg "LiveConnection" --type rust
```

Expected after Task 18: no matches. If any remain, fix before editing docs.

- [ ] **Step 2: This step is the grep; if it fails, do not edit docs**

- [ ] **Step 3: Rewrite the bird's-eye mermaid and layer rules in `ARCHITECTURE.md` to match the spec (SessionHandle, Capabilities, driver-owned SQL, services session_id)**

Quote the spec crate table. State that `DatabaseKind` is a label and connect-form selector only.

- [ ] **Step 4: Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`**

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add ARCHITECTURE.md AGENTS.md
git commit -m "docs: describe capability sessions and driver-owned SQL"
```

---

## Self-review notes (author)

Spec coverage:

- Crate boundaries → Tasks 3, 6, 7, 13, 15, 16, 18
- SessionHandle / capabilities / enum death → Tasks 1–7, 18
- Data flow connect/registry/query/explorer/acp/errors → Tasks 5–6, 9, 15, 16
- Migration phases 1–6 → Tasks 1–3 / 4–7 / 8–13 / 14–16 / 17 / 18–19
- FakeDriver, SessionNotFound, capability invariants → Tasks 3, 6, 18
- ClickHouse CSV without row editing → Global Constraints + Tasks 1, 13, 17

Spec deviation (documented): `import_csv` does not imply `row_editing`.
