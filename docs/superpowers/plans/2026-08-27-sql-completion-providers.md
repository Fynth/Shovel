# SQL Editor Completions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the SQL editor a Zed-style completion menu (keywords + schema) and catalog-backed ghost text at the real caret, with Tab/Enter/Escape and Alt+] / Alt+[ variant cycling.

**Architecture:** Local list providers and keyboard/variant logic live under `ui/src/completion/`. Ghost HTTP goes through `acp-core::stream_native_completion` re-exported by `services` — `ui` must not call provider URLs. `SqlCompletionSettings` on `AppUiSettings` is independent of chat `ActiveModel`. The custom textarea stays.

**Tech Stack:** Rust nightly (workspace pin), Dioxus 0.7, serde, reqwest (acp-core only), tokio mpsc, tree-sitter-sequel, grass SCSS.

**Spec:** `docs/superpowers/specs/2026-08-27-sql-completion-providers-design.md`

## Global Constraints

- Dioxus 0.7 only (`use_signal`, `use_effect`, `#[component]`). No `cx` / `Scope` / `use_state`.
- Never hold a signal read/write across `.await`.
- `ui` may import `models` and `services` only. After Task 7, `ui` must not `use reqwest`.
- `stream_native_completion` must not read or write `NATIVE_CHAT_CANCEL`.
- ACP slugs (`acp:*`) are never completion providers.
- No auto-apply of ghost text. `ai_auto_apply_completions` stays in serde, default `false`, ignored by the editor.
- No CodeStral FIM path. No fallback chain across providers.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- CI: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Do not replace the textarea editor. Do not touch `ui/src/components/inline_completion.rs` unless it fails to compile.

## File structure

- Modify: `models/src/settings.rs` — `SqlCompletionSettings`, field on `AppUiSettings`, default auto-apply `false`
- Modify: `models/src/settings_roundtrip.rs` — missing `sql_completion` → empty provider
- Modify: `models/src/ai_catalog.rs` — `sql_ghost_ready` helper (or method on `AppUiSettings`)
- Modify: `acp-core/src/native_chat.rs` — `CompletionToken`, `completion_request_body`, `stream_native_completion`
- Modify: `acp-core/src/lib.rs`, `acp/src/lib.rs`, `services/src/lib.rs` — re-exports
- Modify: `services/tests/facade_smoke.rs` — mention `stream_native_completion`
- Replace: `ui/src/completion.rs` → `ui/src/completion/mod.rs` plus submodules
- Create: `ui/src/completion/{query,keywords,schema,rank,keyboard,variants,trim,ai}.rs`
- Create: `ui/src/screens/workspace/components/sql_editor/completion_menu.rs`
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` — wire menu + ghost, real caret, no auto-apply
- Modify: `ui/src/screens/workspace/components/sql_editor/highlight.rs` — before + ghost + after
- Modify: `styles/components/_editor.scss` — caret-anchored menu, non-italic ghost
- Modify: `ui/src/app_state/mod.rs` — `set_sql_completion_provider` / `set_sql_completion_model`
- Modify: `ui/src/layout/settings_modal/{mod,sections}.rs` — replace CodeStral section, drop auto-apply toggle
- Modify: `ui/Cargo.toml` — drop `reqwest` after Task 7
- Modify: `docs/ui-description.md` section 7.3 / 7.5

---

### Task 1: SqlCompletionSettings and auto-apply default

**Files:**
- Modify: `models/src/settings.rs`
- Modify: `models/src/settings_roundtrip.rs`
- Modify: `models/src/ai_catalog.rs` only if `sql_ghost_ready` is placed there; otherwise keep the method on `AppUiSettings` in `settings.rs`

**Interfaces:**
- Consumes: existing `AppUiSettings`, `is_native_http_ready`, `lm_api_key`.
- Produces:
  - `pub struct SqlCompletionSettings { pub provider: String, pub model: String }` with `#[serde(default)]`, `Default` both empty strings
  - `AppUiSettings.sql_completion: SqlCompletionSettings`
  - `AppUiSettings::default().ai_auto_apply_completions == false`
  - `impl AppUiSettings { pub fn sql_ghost_ready(&self) -> bool }`

- [ ] **Step 1: Write the failing tests**

In `models/src/settings.rs` under the existing `#[cfg(test)]` module, replace the two auto-apply tests and add:

```rust
#[test]
fn fresh_default_ai_auto_apply_completions_is_disabled() {
    assert!(!AppUiSettings::default().ai_auto_apply_completions);
}

#[test]
fn legacy_settings_missing_ai_auto_apply_completions_defaults_to_false() {
    let settings: AppUiSettings =
        serde_json::from_str(r#"{"theme":"Dark"}"#).expect("legacy JSON");
    assert!(!settings.ai_auto_apply_completions);
}

#[test]
fn fresh_default_sql_completion_is_empty() {
    let settings = AppUiSettings::default();
    assert!(settings.sql_completion.provider.is_empty());
    assert!(settings.sql_completion.model.is_empty());
    assert!(!settings.sql_ghost_ready());
}

#[test]
fn sql_ghost_ready_rejects_acp_slug() {
    let mut settings = AppUiSettings::default();
    settings.ai_features_enabled = true;
    settings.sql_completion.provider = "acp:opencode".into();
    settings.sql_completion.model = "x".into();
    assert!(!settings.sql_ghost_ready());
}
```

In `models/src/settings_roundtrip.rs`:

```rust
#[test]
fn missing_sql_completion_deserializes_to_empty_provider() {
    let parsed: AppUiSettings =
        serde_json::from_value(json!({"theme": "Dark"})).expect("legacy JSON");
    assert!(parsed.sql_completion.provider.is_empty());
    assert!(parsed.sql_completion.model.is_empty());
}
```

In `toggle_single_field_round_trip_preserves_all_persisted_fields` (`models/src/settings.rs`), set a non-default `sql_completion` on the fixture and add:

```rust
assert_eq!(
    reloaded.sql_completion, settings.sql_completion,
    "{field_name} toggle dropped sql_completion"
);
```

Flip the comment that says the auto-apply default is `true`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p models fresh_default_sql_completion_is_empty -- --nocapture`

Expected: FAIL compiling (`sql_completion` / `sql_ghost_ready` not found) or the old auto-apply tests still assert `true`.

- [ ] **Step 3: Write minimal implementation**

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SqlCompletionSettings {
    pub provider: String,
    pub model: String,
}
```

Add `pub sql_completion: SqlCompletionSettings` to `AppUiSettings`. In `Default`, set `sql_completion: SqlCompletionSettings::default()` and `ai_auto_apply_completions: false`.

```rust
impl AppUiSettings {
    pub fn sql_ghost_ready(&self) -> bool {
        self.ai_features_enabled
            && !self.sql_completion.provider.trim().is_empty()
            && !self.sql_completion.model.trim().is_empty()
            && crate::is_native_http_ready(
                &self.sql_completion.provider,
                &self.lm_api_key(&self.sql_completion.provider),
                &self.ai_catalog,
            )
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p models --lib`

Expected: PASS. Fix any other tests that still assume auto-apply defaults to `true`.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs models/src/settings_roundtrip.rs models/src/ai_catalog.rs
git commit -m "Add SqlCompletionSettings and default auto-apply off."
```

---

### Task 2: Isolated native completion stream

**Files:**
- Modify: `acp-core/src/native_chat.rs`
- Modify: `acp-core/src/lib.rs`
- Modify: `acp/src/lib.rs`
- Modify: `services/src/lib.rs`
- Modify: `services/tests/facade_smoke.rs`

**Interfaces:**
- Consumes: `NativeChatRequest`, existing `chat_url`, `auth_headers`, SSE/NDJSON parsers. Does not call `native_chat_cancel_requested` / `request_native_chat_cancel` / `clear_native_chat_cancel`.
- Produces:
  - `pub enum CompletionToken { Text(String), Done, Error(String) }`
  - `pub fn completion_request_body(req: &NativeChatRequest) -> serde_json::Value`
  - `pub fn stream_native_completion(req: NativeChatRequest) -> tokio::sync::mpsc::UnboundedReceiver<CompletionToken>`
  - Re-exports from `acp-core`, `acp`, `services`

- [ ] **Step 1: Write the failing tests**

In `acp-core/src/native_chat.rs` `#[cfg(test)]`:

```rust
#[test]
fn completion_request_body_sets_max_tokens_temperature_and_stop() {
    let req = NativeChatRequest {
        base_url: "https://api.openai.com".into(),
        api_key: "sk".into(),
        model: "gpt-4o-mini".into(),
        messages: vec![NativeChatMessage {
            role: "user".into(),
            content: "select ".into(),
        }],
        provider_slug: "openai".into(),
        thinking_enabled: false,
        reasoning_effort: "medium".into(),
    };
    let body = completion_request_body(&req);
    assert_eq!(body["model"], "gpt-4o-mini");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 100);
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["stop"][0], "\n\n");
    assert_eq!(body["stop"][1], "```");
    assert!(body.get("thinking").is_none());
}

#[test]
fn completion_ollama_body_uses_options_not_chat_cancel() {
    request_native_chat_cancel();
    let req = NativeChatRequest {
        base_url: "http://localhost:11434".into(),
        api_key: String::new(),
        model: "qwen2.5-coder".into(),
        messages: vec![],
        provider_slug: "ollama".into(),
        thinking_enabled: false,
        reasoning_effort: "medium".into(),
    };
    let body = completion_request_body(&req);
    clear_native_chat_cancel();
    assert_eq!(body["options"]["num_predict"], 100);
    assert_eq!(body["options"]["temperature"], 0.2);
    assert_eq!(body["options"]["stop"][0], "\n\n");
}

#[test]
fn map_native_event_to_completion_ignores_thoughts() {
    assert_eq!(
        completion_token_from_event(NativeChatEvent::Thought("hmm".into())),
        None
    );
    assert_eq!(
        completion_token_from_event(NativeChatEvent::Delta("FROM".into())),
        Some(CompletionToken::Text("FROM".into()))
    );
    assert_eq!(
        completion_token_from_event(NativeChatEvent::Finished),
        None
    );
    assert_eq!(
        completion_token_from_event(NativeChatEvent::Error("nope".into())),
        Some(CompletionToken::Error("nope".into()))
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p acp-core completion_request_body_sets_max_tokens -- --nocapture`

Expected: FAIL (`completion_request_body` not found).

- [ ] **Step 3: Write minimal implementation**

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionToken {
    Text(String),
    Done,
    Error(String),
}

const COMPLETION_MAX_TOKENS: u32 = 100;
const COMPLETION_TEMPERATURE: f64 = 0.2;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(15);

pub fn completion_request_body(req: &NativeChatRequest) -> Value {
    let stop = json!(["\n\n", "```"]);
    if req.provider_slug == "ollama" {
        json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "options": {
                "num_predict": COMPLETION_MAX_TOKENS,
                "temperature": COMPLETION_TEMPERATURE,
                "stop": stop,
            }
        })
    } else {
        json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "max_tokens": COMPLETION_MAX_TOKENS,
            "temperature": COMPLETION_TEMPERATURE,
            "stop": stop,
        })
    }
}

pub fn completion_token_from_event(event: NativeChatEvent) -> Option<CompletionToken> {
    match event {
        NativeChatEvent::Delta(text) => Some(CompletionToken::Text(text)),
        NativeChatEvent::Thought(_) => None,
        NativeChatEvent::Finished => None,
        NativeChatEvent::Error(text) => Some(CompletionToken::Error(text)),
    }
}
```

Implement `stream_native_completion` by copying `stream_native_chat`'s HTTP POST (same `chat_url` / `auth_headers`) with `timeout(COMPLETION_TIMEOUT)`, body from `completion_request_body`, and a byte-stream unfold that does **not** call `native_chat_cancel_requested` or `wait_for_native_cancel`. Map events through `completion_token_from_event` onto an `mpsc::unbounded_channel`. After the stream ends without `Error`/`Done`, send `CompletionToken::Done`. On setup failure, send `CompletionToken::Error`.

Re-export `CompletionToken` and `stream_native_completion` from `acp-core/src/lib.rs`, `acp/src/lib.rs`, and `services/src/lib.rs`. Add `let _ = &services::stream_native_completion;` to `facade_smoke.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p acp-core --lib` and `cargo test -p services --test facade_smoke`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/native_chat.rs acp-core/src/lib.rs acp/src/lib.rs services/src/lib.rs services/tests/facade_smoke.rs
git commit -m "Add isolated native SQL completion stream."
```

---

### Task 3: Completion query and keywords

**Files:**
- Create: `ui/src/completion/query.rs`
- Create: `ui/src/completion/keywords.rs`
- Create: `ui/src/completion/mod.rs` (thin `pub mod` file; do not delete `ui/src/completion.rs` until Task 7)

Rust will not allow both `completion.rs` and `completion/`. Keep new modules as `ui/src/completion_engine/` until Task 7 **or** move the existing file first.

Do this: in this task, create `ui/src/completion/` by renaming the current file to `ui/src/completion/legacy.rs` and adding `mod.rs` that `mod legacy; pub use legacy::*;` plus the new modules. `ui/src/lib.rs` already has `mod completion`.

**Interfaces:**
- Consumes: `models::DatabaseKind`, `sql_editor/selection.rs` `current_token_range` / `EditorSelection` (duplicate a small token scan in `query.rs` so `completion` does not depend on the editor module).
- Produces:
  - `pub enum CompletionClause { From, Column, Call, Other }`
  - `pub struct CompletionQuery { pub sql: String, pub cursor: usize, pub token: String, pub token_range: Range<usize>, pub clause: CompletionClause, pub dotted: Vec<String> }`
  - `pub fn parse_completion_query(sql: &str, cursor: usize) -> CompletionQuery`
  - `pub enum CompletionKind { Keyword, Schema, Table, View, Column, Function, Procedure }`
  - `pub struct CompletionItem { pub label: String, pub detail: String, pub kind: CompletionKind, pub replace: Range<usize> }`
  - `pub fn keyword_items(kind: DatabaseKind, query: &CompletionQuery) -> Vec<CompletionItem>`
  - `pub fn match_keyword_case(keyword: &str, typed: &str) -> String`

- [ ] **Step 1: Write the failing tests**

In `ui/src/completion/query.rs`:

```rust
#[test]
fn parse_from_clause_after_from() {
    let q = parse_completion_query("SELECT * FROM us", 16);
    assert_eq!(q.clause, CompletionClause::From);
    assert_eq!(q.token, "us");
    assert!(q.dotted.is_empty());
}

#[test]
fn parse_dotted_table_column() {
    let sql = "SELECT * FROM users.";
    let q = parse_completion_query(sql, sql.len());
    assert_eq!(q.dotted, vec!["users".to_string()]);
    assert!(q.token.is_empty());
    assert_eq!(q.token_range, sql.len()..sql.len());
}

#[test]
fn parse_select_clause() {
    let sql = "SELECT nam";
    let q = parse_completion_query(sql, sql.len());
    assert_eq!(q.clause, CompletionClause::Column);
    assert_eq!(q.token, "nam");
}

#[test]
fn parse_default_other() {
    let q = parse_completion_query("SEL", 3);
    assert_eq!(q.clause, CompletionClause::Other);
    assert_eq!(q.token, "SEL");
}
```

In `ui/src/completion/keywords.rs`:

```rust
#[test]
fn keyword_case_follows_typed_prefix() {
    assert_eq!(match_keyword_case("SELECT", "sel"), "select");
    assert_eq!(match_keyword_case("SELECT", "SEL"), "SELECT");
    assert_eq!(match_keyword_case("SELECT", "Sel"), "SELECT");
}

#[test]
fn postgres_includes_ilike() {
    let q = parse_completion_query("SELECT * FROM t WHERE name IL", 30);
    let items = keyword_items(DatabaseKind::Postgres, &q);
    assert!(items.iter().any(|item| item.label.eq_ignore_ascii_case("ILIKE")));
}

#[test]
fn clickhouse_includes_engine() {
    let q = parse_completion_query("CREATE TABLE t EN", 17);
    let items = keyword_items(DatabaseKind::ClickHouse, &q);
    assert!(items.iter().any(|item| item.label.eq_ignore_ascii_case("ENGINE")));
}
```

Adjust the `IL` cursor to `sql.len()` in the real test using a `let sql = "..."` binding so the index cannot drift.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui parse_from_clause_after_from -- --nocapture`

Expected: FAIL (module / function missing).

- [ ] **Step 3: Write minimal implementation**

`parse_completion_query`: clamp cursor to a char boundary. Token = identifier `[A-Za-z_][A-Za-z0-9_]*` immediately left of the cursor (empty if the previous char is `.` or non-identifier). `dotted`: walk left over `.ident` chains (`users.` → `["users"]`, `public.users.` → `["public","users"]`). `clause`: scan the last SQL keyword before the token/dot, skipping comments is not required in this cycle. Map:

- `FROM` | `JOIN` | `INTO` | `UPDATE` | `TABLE` → `From`
- `SELECT` | `WHERE` | `SET` | `ON` | `BY` (for `GROUP BY` / `ORDER BY`) → `Column`
- previous non-space char is `(` → `Call`
- else `Other`

`match_keyword_case`: if `typed` is non-empty and all lowercase → lowercase keyword; all uppercase → uppercase; otherwise the keyword's stored uppercase form.

`keyword_items`: union of a shared list (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `FROM`, `WHERE`, `JOIN`, `INNER`, `LEFT`, `RIGHT`, `OUTER`, `ON`, `AS`, `AND`, `OR`, `NOT`, `IN`, `IS`, `NULL`, `LIKE`, `BETWEEN`, `ORDER`, `GROUP`, `HAVING`, `LIMIT`, `OFFSET`, `CREATE`, `ALTER`, `DROP`, `TABLE`, `INDEX`, `VIEW`, `INTO`, `VALUES`, `SET`, `DISTINCT`, `UNION`, `ALL`, `CASE`, `WHEN`, `THEN`, `ELSE`, `END`, `WITH`, `EXISTS`) plus dialect extras:

- Postgres: `ILIKE`, `RETURNING`, `LATERAL`
- MySQL: `AUTO_INCREMENT`, `ENGINE`
- ClickHouse: `ENGINE`, `PREWHERE`, `SETTINGS`, `FINAL`
- SQLite: `AUTOINCREMENT`, `PRAGMA`

Each item: `kind: Keyword`, `detail: "keyword"`, `replace: query.token_range`, `label: match_keyword_case(...)`. Do not filter here; Task 4 filters.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui parse_from_clause_after_from parse_dotted_table_column parse_select_clause parse_default_other keyword_case_follows_typed_prefix postgres_includes_ilike clickhouse_includes_engine -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion ui/src/completion.rs
git commit -m "Add SQL completion query parsing and keyword items."
```

---

### Task 4: Schema items, filter, rank, cap

**Files:**
- Create: `ui/src/completion/schema.rs`
- Create: `ui/src/completion/rank.rs`
- Modify: `ui/src/completion/mod.rs`

**Interfaces:**
- Consumes: `CompletionQuery`, `CompletionItem`, `CompletionKind`, `CompletionClause`, `models::ExplorerNode`, `ExplorerNodeKind`.
- Produces:
  - `pub fn schema_items(nodes: &[ExplorerNode], query: &CompletionQuery) -> Vec<CompletionItem>`
  - `pub fn filter_and_rank(items: Vec<CompletionItem>, query: &CompletionQuery) -> Vec<CompletionItem>` (cap 50)
  - `pub fn collect_menu_items(kind: DatabaseKind, nodes: &[ExplorerNode], query: &CompletionQuery, force: bool) -> Vec<CompletionItem>`
  - `pub fn merge_columns_into_tree(nodes: &mut [ExplorerNode], schema: Option<&str>, table: &str, columns: &[String])`
  - `pub fn apply_menu_item(sql: &str, item: &CompletionItem) -> (String, usize)`

- [ ] **Step 1: Write the failing tests**

```rust
fn table(name: &str, columns: &[&str]) -> ExplorerNode {
    ExplorerNode {
        name: name.into(),
        kind: ExplorerNodeKind::Table,
        schema: None,
        qualified_name: name.into(),
        row_count: None,
        children: columns
            .iter()
            .map(|col| ExplorerNode {
                name: (*col).into(),
                kind: ExplorerNodeKind::Column,
                schema: None,
                qualified_name: format!("{name}.{col}"),
                row_count: None,
                children: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn from_clause_ranks_tables_above_columns() {
    let nodes = vec![table("users", &["name"]), table("orders", &[])];
    let query = parse_completion_query("SELECT * FROM u", 15);
    let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
    assert_eq!(items[0].label, "users");
    assert_eq!(items[0].kind, CompletionKind::Table);
}

#[test]
fn dotted_prefix_prefers_columns() {
    let nodes = vec![table("users", &["id", "name"])];
    let sql = "SELECT * FROM users.";
    let query = parse_completion_query(sql, sql.len());
    let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
    assert!(items.iter().all(|item| item.kind == CompletionKind::Column));
    assert!(items.iter().any(|item| item.label == "name"));
}

#[test]
fn filter_prefix_beats_substring_and_caps_at_50() {
    let mut items: Vec<CompletionItem> = (0..80)
        .map(|i| CompletionItem {
            label: format!("col{i:02}"),
            detail: String::new(),
            kind: CompletionKind::Column,
            replace: 0..0,
        })
        .collect();
    items.push(CompletionItem {
        label: "id".into(),
        detail: String::new(),
        kind: CompletionKind::Column,
        replace: 0..0,
    });
    let query = parse_completion_query("SELECT i", 8);
    let ranked = filter_and_rank(items, &query);
    assert_eq!(ranked[0].label, "id");
    assert!(ranked.len() <= 50);
}

#[test]
fn empty_token_without_force_or_dot_is_empty() {
    let nodes = vec![table("users", &[])];
    let query = parse_completion_query("SELECT ", 7);
    let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
    assert!(items.is_empty());
}

#[test]
fn apply_menu_item_replaces_token() {
    let sql = "SELECT * FROM us";
    let query = parse_completion_query(sql, sql.len());
    let item = CompletionItem {
        label: "users".into(),
        detail: String::new(),
        kind: CompletionKind::Table,
        replace: query.token_range.clone(),
    };
    let (next, cursor) = apply_menu_item(sql, &item);
    assert_eq!(next, "SELECT * FROM users");
    assert_eq!(cursor, next.len());
}

#[test]
fn merge_columns_into_tree_appends_column_children() {
    let mut nodes = vec![table("users", &[])];
    merge_columns_into_tree(&mut nodes, None, "users", &["id".into(), "name".into()]);
    assert_eq!(nodes[0].children.len(), 2);
    assert_eq!(nodes[0].children[0].kind, ExplorerNodeKind::Column);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui from_clause_ranks_tables_above_columns -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Walk `nodes` recursively. Emit schema / table / view / function / procedure / column items with `detail` like `schema.table` or `table.column`. When `query.dotted` is non-empty, only emit columns of the matching table/view (last dotted segment is the table, optional previous is schema). `replace` is `query.token_range`.

`filter_and_rank`: drop items whose label does not contain `token` case-insensitively (empty token keeps all). Score: prefix 200, else substring 100; +50 if `kind` is preferred for `query.clause` (From: Schema/Table/View; dotted non-empty: Column; Column: Column then Table; Call: Function/Procedure; Other: Keyword/Table/View). Sort by score descending, then label length ascending, then label case-insensitive. Truncate to 50.

`collect_menu_items`: if `!force && query.token.is_empty() && query.dotted.is_empty()` return empty. Else concatenate `keyword_items` + `schema_items` and `filter_and_rank`.

`apply_menu_item`: `format!("{}{}{}", &sql[..item.replace.start], item.label, &sql[item.replace.end..])`, cursor = `item.replace.start + item.label.len()`.

`merge_columns_into_tree`: find the table/view by name (and schema when `Some`), replace `children` with `Column` nodes when current children have no columns.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui from_clause_ranks_tables_above_columns dotted_prefix_prefers_columns filter_prefix_beats_substring_and_caps_at_50 empty_token_without_force_or_dot_is_empty apply_menu_item_replaces_token merge_columns_into_tree_appends_column_children -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion
git commit -m "Add schema completion items, ranking, and token replace."
```

---

### Task 5: Keyboard dispatch and ghost variant ring

**Files:**
- Create: `ui/src/completion/keyboard.rs`
- Create: `ui/src/completion/variants.rs`
- Modify: `ui/src/completion/mod.rs`

**Interfaces:**
- Consumes: nothing from Dioxus.
- Produces:
  - `pub enum CompletionKey { Escape, Tab, ShiftTab, Enter, ArrowUp, ArrowDown, Character(char), CtrlSpace, AltRBracket, AltLBracket, Other }`
  - `pub enum EditorKeyAction { Pass, CloseMenu, DismissGhost, CycleGhostNext, CycleGhostPrev, MenuMove(i32), AcceptMenu, AcceptGhost, Indent { shift: bool }, ForceMenu }`
  - `pub fn editor_completion_action(key: CompletionKey, menu_open: bool, ghost_visible: bool) -> EditorKeyAction`
  - `pub struct GhostVariants { snapshot: usize, items: Vec<String>, index: usize }` with `set_first`, `current`, `prev`, `show_next_existing`, `needs_fetch`, `push`, `clear_if_changed`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn keyboard_priority_table() {
    use CompletionKey::*;
    use EditorKeyAction::*;
    assert_eq!(
        editor_completion_action(Escape, true, true),
        CloseMenu
    );
    assert_eq!(
        editor_completion_action(Escape, false, true),
        DismissGhost
    );
    assert_eq!(
        editor_completion_action(Tab, true, true),
        AcceptMenu
    );
    assert_eq!(
        editor_completion_action(Tab, false, true),
        AcceptGhost
    );
    assert_eq!(
        editor_completion_action(Tab, false, false),
        Indent { shift: false }
    );
    assert_eq!(
        editor_completion_action(Enter, true, true),
        AcceptMenu
    );
    assert_eq!(
        editor_completion_action(Enter, false, true),
        Pass
    );
    assert_eq!(
        editor_completion_action(ArrowUp, true, false),
        MenuMove(-1)
    );
    assert_eq!(
        editor_completion_action(ArrowDown, true, false),
        MenuMove(1)
    );
    assert_eq!(
        editor_completion_action(AltRBracket, true, true),
        CycleGhostNext
    );
    assert_eq!(
        editor_completion_action(AltLBracket, false, true),
        CycleGhostPrev
    );
    assert_eq!(
        editor_completion_action(AltRBracket, true, false),
        Pass
    );
    assert_eq!(
        editor_completion_action(CtrlSpace, false, false),
        ForceMenu
    );
    assert_eq!(
        editor_completion_action(ShiftTab, true, true),
        Indent { shift: true }
    );
}

#[test]
fn variant_ring_next_fetch_prev_and_snapshot_clear() {
    let mut ring = GhostVariants::default();
    ring.set_first(1, "FROM users".into());
    assert_eq!(ring.current(), Some("FROM users"));
    assert!(ring.needs_fetch());
    assert!(!ring.show_next_existing());
    ring.push("FROM orders".into());
    assert_eq!(ring.current(), Some("FROM orders"));
    ring.prev();
    assert_eq!(ring.current(), Some("FROM users"));
    ring.prev();
    assert_eq!(ring.current(), Some("FROM users"));
    ring.clear_if_changed(2);
    assert!(ring.current().is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui keyboard_priority_table -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Evaluate `editor_completion_action` in this order (spec § keyboard):

1. Escape → `CloseMenu` if `menu_open`, else `DismissGhost` if `ghost_visible`, else `Pass`
2. AltRBracket / AltLBracket → cycle if `ghost_visible`, else `Pass`
3. ArrowUp/Down → `MenuMove` if `menu_open`, else `Pass`
4. Tab → `AcceptMenu` if `menu_open`, else `AcceptGhost` if `ghost_visible`, else `Indent { shift: false }`
5. ShiftTab → `Indent { shift: true }` always (never accepts menu or ghost)
6. Enter → `AcceptMenu` if `menu_open` else `Pass`
7. CtrlSpace → `ForceMenu`
8. else `Pass`

`GhostVariants`: `needs_fetch` is `!items.is_empty() && index + 1 >= items.len()`. `show_next_existing` increments index when `index + 1 < items.len()` and returns whether it moved. `push` appends and sets `index` to the new last item. `clear_if_changed` resets when snapshot differs.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui keyboard_priority_table variant_ring_next_fetch_prev_and_snapshot_clear -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion
git commit -m "Add completion keyboard priority and ghost variant ring."
```

---

### Task 6: Highlight split and mid-document trim/apply caret

**Files:**
- Modify: `ui/src/screens/workspace/components/sql_editor/highlight.rs`
- Create: `ui/src/completion/trim.rs` (move `trim_completion_for_cursor` and overlap helpers out of `sql_editor.rs`)
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` — re-export/use the moved trim; fix `apply_inline_completion` caret

**Interfaces:**
- Consumes: existing `trim_completion_for_cursor` behavior.
- Produces:
  - `pub fn inline_highlight_parts(sql: &str, cursor: usize, ghost: Option<&str>) -> (&str, Option<&str>, &str)`
  - `pub fn trim_completion_for_cursor(sql: &str, cursor: usize, completion: &str) -> String` in `ui/src/completion/trim.rs`
  - `apply_inline_completion` uses the DOM selection start, not `sql.len()`

- [ ] **Step 1: Write the failing tests**

In `highlight.rs`:

```rust
#[test]
fn inline_highlight_parts_keeps_suffix_after_caret() {
    let (before, ghost, after) =
        inline_highlight_parts("select  from users", 7, Some("id, name"));
    assert_eq!(before, "select ");
    assert_eq!(ghost, Some("id, name"));
    assert_eq!(after, " from users");
}

#[test]
fn inline_highlight_parts_without_ghost_is_full_sql() {
    let (before, ghost, after) = inline_highlight_parts("select 1", 8, None);
    assert_eq!(before, "select 1");
    assert!(ghost.is_none());
    assert_eq!(after, "");
}
```

Move the existing `trim_completion_removes_repeated_token_and_suffix_overlap` test with the mid-document case:

```rust
#[test]
fn trim_completion_at_mid_document_caret() {
    let sql = "select  from users";
    let cursor = "select ".len();
    assert_eq!(
        trim_completion_for_cursor(sql, cursor, "id, name from users"),
        "id, name"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui inline_highlight_parts_keeps_suffix_after_caret trim_completion_at_mid_document_caret -- --nocapture`

Expected: FAIL (`inline_highlight_parts` missing; mid-document trim may already pass if the function is cursor-aware — if it passes, keep the test).

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn inline_highlight_parts<'a>(
    sql: &'a str,
    cursor: usize,
    ghost: Option<&'a str>,
) -> (&'a str, Option<&'a str>, &'a str) {
    let cursor = cursor.min(sql.len());
    match ghost {
        Some(text) if !text.is_empty() => (&sql[..cursor], Some(text), &sql[cursor..]),
        _ => (sql, None, ""),
    }
}
```

Update `SqlHighlightContent` to highlight `before` and `after` as separate `highlight_sql` runs, with the ghost span between them.

In `apply_inline_completion`, change the `document::eval` destructure to `(sql, start, end)` and set `let cursor = start;` (clamped). Delete `let cursor = actual_sql.len();`. Stop calling `schedule_auto_apply`. Delete `schedule_auto_apply`, `AUTO_APPLY_IDLE_MS`, and the `APP_AI_AUTO_APPLY_COMPLETIONS` import from `sql_editor.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui inline_highlight_parts_keeps_suffix_after_caret trim_completion_at_mid_document_caret completion_request_parts_split_sql_at_cursor -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion ui/src/screens/workspace/components/sql_editor.rs ui/src/screens/workspace/components/sql_editor/highlight.rs
git commit -m "Show SQL after the caret and apply ghost text at the cursor."
```

---

### Task 7: Catalog-backed ghost requests (drop ui reqwest)

**Files:**
- Create: `ui/src/completion/ai.rs`
- Modify: `ui/src/completion/mod.rs` — stop exporting DeepSeek/CodeStral HTTP
- Delete: `ui/src/completion/legacy.rs` (the old `completion.rs` body)
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` — call `stream_sql_ghost` at the real caret
- Modify: `ui/Cargo.toml` — remove `reqwest`

**Interfaces:**
- Consumes: `AppUiSettings::sql_ghost_ready`, `services::{NativeChatRequest, NativeChatMessage, stream_native_completion, CompletionToken}`, catalog base URL + `lm_api_key`.
- Produces:
  - `pub fn stream_sql_ghost(settings: &AppUiSettings, prefix: String, suffix: Option<String>, schema_context: String, avoid: &[String]) -> UnboundedReceiver<CompletionToken>`
  - Ghost prompt: output only raw SQL after the cursor; include schema; if `avoid` is non-empty, instruct not to repeat those strings.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn stream_sql_ghost_without_provider_completes_immediately() {
    let settings = AppUiSettings::default();
    let mut rx = stream_sql_ghost(&settings, "select ".into(), None, String::new(), &[]);
    let token = rx.try_recv().expect("done token");
    assert!(matches!(token, CompletionToken::Done));
}

#[test]
fn ghost_messages_include_avoid_list() {
    let messages = ghost_messages("select ", None, "-- Table: users\n", &["FROM users".into()]);
    let blob = messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(blob.contains("users"));
    assert!(blob.contains("FROM users"));
    assert!(blob.contains("[CURSOR]"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui stream_sql_ghost_without_provider_completes_immediately -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Resolve base URL: builtin `default_base_url` or custom provider `base_url`, then override from `ai_catalog.overrides[slug].base_url` if non-empty, then `normalize_native_chat_url`.

If `!settings.sql_ghost_ready()`, send `Done` on a channel and return.

Otherwise build `NativeChatRequest` with `provider_slug = settings.sql_completion.provider`, `model = settings.sql_completion.model`, `thinking_enabled: false`, and call `services::stream_native_completion`.

In `sql_editor.rs` ghost effect:

- Use `editor_selection` / DOM `(sql, start, end)` as the caret. If `start != end`, skip.
- Require `sql.len() >= 3` and `sql_ghost_ready`.
- Debounce 180ms, request id guard unchanged.
- Pass `prefix = sql[..cursor]`, `suffix = sql[cursor..]`.
- `avoid` is `completion_runtime.variants.items` when cycling (empty on first request).
- On `Text`, accumulate and `set_active` with the **real** cursor.
- On `Error`, `toast_error` once and clear ghost.
- Do not call `schedule_auto_apply`.
- Do not instantiate `CompletionService`.

Remove `reqwest` from `ui/Cargo.toml` if no remaining `use reqwest`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui stream_sql_ghost_without_provider_completes_immediately ghost_messages_include_avoid_list -- --nocapture`

Then: `cargo check -p ui`

Expected: PASS / compile. `ui` must not reference `reqwest`.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion ui/src/screens/workspace/components/sql_editor.rs ui/Cargo.toml
git commit -m "Drive SQL ghost text from the catalog completion stream."
```

---

### Task 8: Menu UI, caret anchor, editor keys, column fetch

**Files:**
- Create: `ui/src/screens/workspace/components/sql_editor/completion_menu.rs`
- Modify: `ui/src/screens/workspace/components/sql_editor.rs`
- Modify: `styles/components/_editor.scss`
- Modify: `ui/src/completion/variants.rs` usage inside `CompletionRuntime`

**Interfaces:**
- Consumes: `collect_menu_items`, `apply_menu_item`, `editor_completion_action`, `GhostVariants`, `services::load_table_columns`, `explorer_sections` signal, `APP_STATE` session `DatabaseKind`.
- Produces: caret-anchored menu RSX; keyboard routing in `onkeydown`; column merge on `table.`.

- [ ] **Step 1: Write the failing tests**

In `completion_menu.rs`:

```rust
#[test]
fn autocomplete_offset_flips_above_when_clipped() {
    let (left, top, flip) = autocomplete_offset(
        40.0, 180.0, 18.0, 120.0, 220.0, 400.0, 240.0,
    );
    assert!(flip);
    assert!(top < 180.0);
    assert!(left >= 0.0);
}

#[test]
fn autocomplete_offset_stays_below_when_space() {
    let (_, top, flip) = autocomplete_offset(
        10.0, 20.0, 18.0, 80.0, 400.0, 400.0, 200.0,
    );
    assert!(!flip);
    assert!(top > 20.0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ui autocomplete_offset_flips_above_when_clipped -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

`autocomplete_offset(caret_x, caret_y, line_height, menu_height, editor_height, editor_width, menu_width) -> (f64, f64, bool)`:

- `left = caret_x.min(editor_width - menu_width).max(0.0)`
- if `caret_y + line_height + menu_height > editor_height` then `top = (caret_y - menu_height).max(0.0)`, `flip = true`
- else `top = caret_y + line_height`, `flip = false`

CSS: `.sql-editor` is `position: relative`. `.sql-editor__autocomplete` is `position: absolute; z-index: 3;` with **no** `left/right/bottom: 12px`. Inline `style` sets `left`/`top`/`max-height`. Keep item/label/detail/kind classes. Ghost: `.sql-editor__token--inline` same mono font, muted color, **no** italic.

JS via `document::eval` against `#workspace-sql-editor`: mirror the textarea value up to the caret in a hidden pre (same font/size/tab-size/white-space) and return `{x, y, lineHeight, editorWidth, editorHeight}` relative to `.sql-editor`.

Menu state on `SqlEditor`: `menu_items: Vec<CompletionItem>`, `menu_index: usize`, `menu_force: bool`, caret coords. Recompute items on `editor_revision` from `parse_completion_query` + `collect_menu_items`. Ctrl-Space sets `menu_force = true`.

`onkeydown`: map Dioxus `Key`/`Code`/`modifiers` to `CompletionKey` (Ctrl/Cmd+Space → `CtrlSpace`; Alt+`]` → `AltRBracket`; Alt+`[` → `AltLBracket`; Shift+Tab → `ShiftTab`). Run `editor_completion_action`. `prevent_default` for CloseMenu, DismissGhost, cycle, MenuMove, AcceptMenu, AcceptGhost, ForceMenu, and Tab when it accepts. Indent still calls `indent_lines_in_active_tab`.

Accept menu: `apply_menu_item`, write DOM with `set_editor_value_script`, `sync_active_tab_sql_draft`, close menu, clear ghost ring.

Accept ghost: existing `apply_inline_completion` with `variants.current()`.

Cycle next: if `show_next_existing()` update `active` text; else `stream_sql_ghost(..., avoid: &variants.items)` and `push` on Done.

When `query.dotted` is non-empty and the matching table has no column children, `spawn` `services::load_table_columns(session_id, schema, table)`. On success, if the query snapshot still matches, `merge_columns_into_tree` on `explorer_sections`. On error, do nothing.

Extend `CompletionRuntime` with `GhostVariants` and `discarded: bool`. Typing clears variants and discarded. Escape with no menu sets `discarded = true` and clears `active`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ui autocomplete_offset_flips_above_when_clipped autocomplete_offset_stays_below_when_space keyboard_priority_table -- --nocapture`

Then: `cargo check -p ui`

Expected: PASS / compile.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/sql_editor ui/src/completion styles/components/_editor.scss
git commit -m "Wire Zed-style SQL completion menu and caret-anchored popup."
```

---

### Task 9: Settings UI, app_state helpers, docs

**Files:**
- Modify: `ui/src/app_state/mod.rs`
- Modify: `ui/src/layout/settings_modal/mod.rs`
- Modify: `ui/src/layout/settings_modal/sections.rs`
- Modify: `docs/ui-description.md`

**Interfaces:**
- Consumes: `SqlCompletionSettings`, `builtin_providers`, `resolve_picker_models`, `is_native_http_ready` / `sql_ghost_ready`.
- Produces:
  - `pub fn set_sql_completion_provider(provider: String)`
  - `pub fn set_sql_completion_model(model: String)`
  - Settings Editor section: provider `<select onchange>` (native HTTP ready providers only, plus empty “Off”), model `<select onchange>` from that provider’s catalog list
  - No CodeStral section, no auto-apply checkbox

- [ ] **Step 1: Write the failing tests**

In `models/src/settings.rs` `#[cfg(test)]`:

```rust
#[test]
fn sql_completion_choices_exclude_acp_and_disabled() {
    let mut settings = AppUiSettings::default();
    settings.ai_catalog.overrides.insert(
        "deepseek".into(),
        crate::AiProviderOverride {
            enabled: true,
            ..crate::AiProviderOverride::default()
        },
    );
    settings.set_lm_api_key("deepseek", "sk-test".into());
    let choices = settings.sql_completion_choices();
    assert!(choices.iter().any(|c| c.id == "deepseek"));
    assert!(choices.iter().all(|c| !c.id.starts_with("acp:")));
}
```

```rust
pub struct SqlCompletionChoice {
    pub id: String,
    pub label: String,
}

impl AppUiSettings {
    pub fn sql_completion_choices(&self) -> Vec<SqlCompletionChoice> { ... }
}
```

Include Ollama when enabled even with an empty key. Include `custom:*` entries that have a key.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p models sql_completion_choices_exclude_acp_and_disabled -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Helpers in `app_state/mod.rs`:

```rust
pub fn set_sql_completion_provider(provider: String) {
    update_ui_settings(|current| {
        current.sql_completion.provider = provider;
    });
}

pub fn set_sql_completion_model(model: String) {
    update_ui_settings(|current| {
        current.sql_completion.model = model;
    });
}
```

Replace `CodeStralCompletionSection` with `SqlCompletionSection`:

- Hint: “Tab accepts, Escape dismisses, Alt+] / Alt+[ cycle variants. Menu works without AI.”
- Provider `<select>` `onchange` (not `oninput`): first option value `""` label `Off`, then `sql_completion_choices`.
- Model `<select>` `onchange` of `resolve_picker_models` for the selected provider; disabled when provider is empty.
- Changing provider that does not contain the current model sets model to the first listed id (or empty).

Remove the auto-apply toggle from the AI features section.

Delete `CodeStralCompletionSection` and its import/render in `settings_modal/mod.rs`.

Update `docs/ui-description.md` §7.3 and §7.5:

- Menu: keywords + schema, caret popup, Tab/Enter, Ctrl-Space, Escape.
- Ghost: `sql_completion` provider/model, stream at caret, Tab if menu closed, Escape, Alt+] / Alt+[, no auto-apply.
- Remove CodeStral/DeepSeek fallback and auto-apply idle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p models sql_completion_choices_exclude_acp_and_disabled -- --nocapture`

Then: `cargo test -p models --lib` and `cargo test -p ui --lib`

Then: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/app_state/mod.rs ui/src/layout/settings_modal models/src/settings.rs docs/ui-description.md
git commit -m "Add SQL completion provider settings and update editor docs."
```

---

## Manual check (after Task 8–9, not a commit)

Desktop app (`cargo run -p app --features desktop`):

1. No provider: type `SEL` → menu with `SELECT`; Tab inserts; Enter with menu open inserts; Escape closes menu.
2. `SELECT * FROM` + table name prefix → tables ranked first.
3. `users.` loads columns if missing; menu shows columns.
4. Configure a native HTTP completion model. Type a 3+ char prefix. Ghost appears at the caret; text after the caret stays. Tab accepts. Escape dismisses. Alt+] fetches another variant.
5. Auto-apply does not insert. Chat cancel does not kill ghost; typing does.
