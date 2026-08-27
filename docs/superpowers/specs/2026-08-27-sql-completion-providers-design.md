# SQL editor completions (menu + Zed-style ghost text) — design

**Date:** 2026-08-27
**Status:** Draft (awaiting user review)
**Path:** Architectural (provider trait + orchestrator in `ui`, native HTTP stream in `acp-core` via `services`)

The AI catalog spec left inline SQL completion on the legacy DeepSeek/CodeStral
path. This document replaces that path and adds a Zed-style completion menu.

## Problem

The SQL editor has two incomplete completion surfaces:

1. **AI ghost text** is hardcoded to DeepSeek then CodeStral, always requested
   at the *end* of the document, hides SQL after the cursor while a suggestion
   is showing, and auto-inserts after 400ms. That is not Zed.
2. **A completion menu** was styled (`.sql-editor__autocomplete`) but never
   wired. There is no keyword/schema list, no caret anchoring, no keyboard
   routing.

The AI catalog already has native HTTP providers (OpenAI, Groq, OpenRouter,
xAI, Mistral, DeepSeek, Ollama, `custom:*`). Chat uses that catalog. Completion
does not.

## Goal

- **Menu** (local, no network): SQL keywords + schema objects from the explorer
  tree, Zed-like popup at the caret, Tab/Enter accept, Escape dismisses.
- **Ghost text** (network): inline AI suggestion at the real cursor, streaming,
  remaining SQL stays visible, Tab accepts only when the menu is closed,
  Escape dismisses, **no auto-apply**.
- **Separate completion model**: `sql_completion.provider` +
  `sql_completion.model` are independent of the chat `ActiveModel`.
- **Variant cycle**: Alt+] fetches the next ghost variant on demand; Alt+[
  walks back over already fetched variants.
- Menu works with AI features off. Ghost text requires AI features plus a
  configured native HTTP completion provider with a key.

## Non-goals

- Replacing the textarea/highlight editor with CodeMirror, Monaco, or Zed's
  editor.
- Language servers / LSP.
- ACP agents (`acp:*`) as completion providers.
- CodeStral FIM or other provider-specific completion endpoints. All AI
  completions use the existing OpenAI-compatible / Ollama chat stream.
- Snippet placeholders, Copilot-style partial accept (Ctrl-Right), or
  requesting N variants up front.
- Sharing cancel state with agent-panel chat.

## Approach

Chosen: **provider trait + orchestrator**, keep the custom textarea.

Rejected:

- Bolt the menu and catalog into `sql_editor.rs` / `completion.rs` as they
  stand. That file is already a hotspot; HTTP would stay in `ui` against the
  catalog spec (`ui` must not call provider URLs).
- Replace the editor widget. Out of scope for this cycle.

## Architecture

Layer rules from `ARCHITECTURE.md` and the catalog spec stay:

| Piece | Crate | Notes |
| --- | --- | --- |
| `SqlCompletionSettings` | `models` | Persisted on `AppUiSettings` |
| `stream_native_completion` | `acp-core` | Isolated from `NATIVE_CHAT_CANCEL` |
| Facade re-export | `services` | `ui` calls this, not `reqwest` |
| Orchestrator, local providers, menu/ghost UI | `ui` | `ui/src/completion/` + `sql_editor` |

Two independent channels share keyboard priority, not a request pipeline:

```text
keystroke
  ├─ list providers (keywords, schema) → menu at caret
  └─ debounce 180ms → AI stream → ghost at caret

Tab / Enter  → menu item if menu open, else Tab → ghost, else indent / newline
Escape       → close menu if open, else dismiss ghost
Alt+] / Alt+[ → cycle ghost variants (menu unchanged)
```

`ui/src/completion.rs` is split into a module. `InlineCompletion` in
`ui/src/components/inline_completion.rs` is unused by the editor and is not
the render path (ghost text is a highlight-layer span). Leave that component
alone unless a compile warning forces a cleanup.

### AI stream (services / acp-core)

Add `stream_native_completion(req) -> UnboundedReceiver<CompletionToken>`
that:

- Reuses `chat_url` / auth / SSE parsing from `native_chat`.
- Sends `max_tokens: 100`, `temperature: 0.2`, `stop: ["\n\n", "```"]`.
- Does **not** read or write `NATIVE_CHAT_CANCEL`. Dropping the consumer /
  aborting the task is cancellation.
- Uses a 15s timeout (chat stays at 120s).
- Ignores `reasoning_content` / thought events; only `content` deltas become
  `CompletionToken::Text`.

`NativeChatRequest` stays the chat type. Completion builds one from catalog
lookup (base URL, key via `lm_api_key`, model, slug) plus a SQL-only prompt
(schema context + prefix/suffix around the cursor). No FIM body.

ACP slugs are rejected at the settings picker and at request build time.

## Components

### Settings (`models`)

```rust
pub struct SqlCompletionSettings {
    /// Catalog provider slug (`deepseek`, `openai`, `custom:…`). Empty = ghost text off.
    pub provider: String,
    pub model: String,
}
```

Persisted as `AppUiSettings.sql_completion`. Empty provider means ghost text
is disabled; the menu still runs.

`ai_auto_apply_completions` remains in serde (default **false**) so old JSON
loads, but the editor **never auto-inserts** and the settings toggle is
removed.

No automatic migration from CodeStral/DeepSeek completion flags. The user
picks a completion provider in Settings. The CodeStral-specific settings
block is removed from the UI. The `codestral` field stays on
`AppUiSettings` for serde-compat and is not read by completion.

Settings UI (Language models / SQL editor section):

- Provider select: native HTTP catalog providers that are enabled and have a
  key (Ollama may have an empty key). No ACP entries.
- Model select/input: models from that provider's catalog list.
- Helper copy: Tab accepts, Escape dismisses, Alt+] / Alt+[ cycle variants.

`app_state` gets `set_sql_completion_provider` / `set_sql_completion_model`.

### List providers (local)

**Keywords.** Static sets keyed by `DatabaseKind` (SQLite / Postgres / MySQL /
ClickHouse). Shared keywords plus dialect extras (`ILIKE`, `RETURNING`,
`ENGINE`, …). Insert the keyword in the case of the typed prefix
(all-caps prefix → `SELECT`; lowercase → `select`; mixed → keyword default
uppercase).

**Schema.** Walk `ExplorerConnectionSection` for the active session:

| Context | Prefer |
| --- | --- |
| After `FROM` / `JOIN` / `INTO` / `UPDATE` / `TABLE` | schemas, tables, views |
| After `table.` or `schema.table.` | columns of that object |
| After `SELECT` / `WHERE` / `SET` / `ON` / `GROUP BY` / `ORDER BY` | columns, then tables |
| After `(` following a likely CALL/function position | functions, procedures |
| Default / Ctrl-Space with empty token | keywords + tables + views |

If the context is `ident.` (or `schema.ident.`) and the matching table/view
has no column children, call `services::load_table_columns` once and merge
those columns into the workspace `explorer_sections` signal for that
session (same node shape as the explorer column toggle). Failure → menu
without columns, no toast.

Filter: case-insensitive prefix first, then substring. Rank: context boost,
then prefix over substring, then shorter label, then alphabetical. Cap at
50 items; show ~12 before scrolling.

Each item: `label`, `detail` (schema, type, or `table.column`), `kind`
(`Keyword`, `Schema`, `Table`, `View`, `Column`, `Function`, `Procedure`).

Replace the current identifier token (or the empty range after `.`) with
`label`. Do not quote identifiers in this cycle.

### Menu UI

- Open automatically when the token at the caret is an identifier (or just
  after `.`) and at least one item matches. Also Ctrl-Space (not Cmd-Space)
  to force, including an empty token.
- Anchor to the caret, not the bottom of the editor. A `document::eval`
  helper returns caret `{x, y, lineHeight}` relative to `.sql-editor`; the
  popup is `position: absolute` under the caret, flipped above if it would
  clip the editor.
- Existing CSS classes stay (`.sql-editor__autocomplete*`); positioning
  rules change from `left/right/bottom: 12px` to caret coordinates.
- Default selection is index 0. ArrowUp/Down move it and
  `prevent_default` so the caret does not move.
- Clicking an item accepts it.

### Ghost text

- Request only when `ai_features_enabled`, `sql_completion.provider` is a
  native HTTP slug, and a key (or Ollama) is available. Minimum prefix: 3
  characters. No request if the selection is a range.
- Debounce 180ms. New revision aborts the in-flight task.
- Complete at `editor_selection` (collapsed caret), **not** `sql.len()`.
- Highlight layer paints: tokens before caret, ghost span
  (`.sql-editor__token--inline`: same mono font, muted color, **not**
  italic), tokens after caret. Ghost may contain newlines.
- Streaming deltas show immediately. Empty trimmed result hides the ghost.
- Provider errors: one toast, no ghost. Do not fall back to another
  provider.

**Variants.** Runtime holds `Vec<String>` + index for the current
SQL+cursor snapshot.

- First successful stream fills `variants[0]`.
- Alt+] : if `index + 1 < len`, show it; else start another request with
  the same prefix/suffix plus an instruction that previous suggestions
  (joined) must not be repeated. Append on success. While fetching, keep
  showing the current variant.
- Alt+[ : `index = index.saturating_sub(1)`.
- Any document/cursor change clears the ring.
- Tab accepts `variants[index]` through the same trim/insert path as today,
  but using the real caret.

### Keyboard priority (editor)

Evaluated in this order on keydown:

1. Existing Ctrl/Cmd shortcuts (Run, Format, Comment, Save, Clear, Explain)
   unchanged.
2. Escape: close menu if open; else dismiss ghost (mark discarded until the
   next document change).
3. Alt+] / Alt+[: if a ghost is visible, cycle variants even when the
   menu is open. If no ghost, ignore.
4. ArrowUp/Down: if menu open, move selection.
5. Tab (no Ctrl/Cmd): if menu open, accept item; else if ghost visible,
   accept ghost; else existing indent/outdent (Shift+Tab outdents).
6. Enter: if menu open, accept item; else default newline.
7. Other keys: default typing; menu refilters; ghost invalidates.

Ctrl-Space opens/refreshes the menu and `prevent_default`.

### Editor extraction

`ui/src/screens/workspace/components/sql_editor.rs` stays the owner of
signals (`draft_sql`, `editor_selection`, menu state, `CompletionRuntime`).
Move caret measurement JS, menu RSX, and list-filter helpers into
`sql_editor/completion_menu.rs`. Fix `SqlHighlightContent` to render the
suffix after the caret when ghost text is present.

`apply_inline_completion` must read the live caret from
`editor_value_and_selection_query_script` (already returns start/end) and
use that offset, not `actual_sql.len()`.

## Data flow

1. `oninput` / selection sync updates SQL + caret.
2. Orchestrator builds `CompletionQuery { sql, cursor, token, clause, dotted }`.
3. Keywords + schema produce `Vec<CompletionItem>` synchronously (column
   fetch may be async; menu updates when it returns if the query snapshot
   still matches).
4. Independently, after 180ms idle, AI stream starts for that snapshot.
5. Accept paths write SQL via the existing `set_editor_value_script` /
   `sync_active_tab_sql_draft` / selection restore.

## Error handling

| Failure | Behavior |
| --- | --- |
| No completion provider / no key | Ghost off; menu still works |
| AI HTTP / parse error | Toast once per request; ghost cleared |
| Empty AI text after trim | Ghost hidden, no toast |
| `load_table_columns` fails | Menu without columns |
| User types during stream | Abort; ignore late tokens (request id) |
| ACP slug in settings | Treat as disabled ghost text |

## Testing

Unit tests (no network):

- Clause detection: `FROM`, `table.`, `SELECT`, default.
- Keyword case matching.
- Schema ranking prefers tables after `FROM` and columns after `dot`.
- Filter prefix vs substring; 50-item cap.
- `trim_completion_for_cursor` at mid-document caret, including suffix
  overlap.
- Highlight split: before + ghost + after (pure function if extracted).
- Keyboard priority table (menu vs ghost vs indent) as a pure function of
  `(menu_open, ghost_visible, key)`.
- Variant ring: Alt+] appends, Alt+[ does not go below 0, snapshot change
  clears.
- `SqlCompletionSettings` serde roundtrip; missing field → empty provider;
  `ai_auto_apply_completions` missing → false.
- `stream_native_completion` body includes `max_tokens` / `stop` and does
  not touch chat cancel (unit-test request JSON; cancel isolation via a
  flag test if the cancel atom is reachable).

CI: existing `cargo test` / clippy / fmt. Desktop UI is not browser-tested;
manual check of caret anchoring and ghost alignment after implementation.

## Files (expected)

- `models/src/settings.rs`, `models/src/settings_roundtrip.rs`
- `ui/src/app_state/` helpers
- `ui/src/layout/settings_modal/sections.rs` (completion provider UI;
  remove CodeStral block and auto-apply toggle)
- `ui/src/completion/` (orchestrator + providers)
- `ui/src/screens/workspace/components/sql_editor.rs` and
  `sql_editor/{highlight,selection,completion_menu}.rs`
- `styles/components/_editor.scss` (caret-anchored menu, non-italic ghost)
- `acp-core/src/native_chat.rs` (or sibling) + `acp-core` / `acp` / `services`
  re-exports
- `docs/ui-description.md` section 7.3 update in the same change set
