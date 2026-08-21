# Learnings - dbeaver-class-polish

## Key architectural facts (from planning exploration)
- `ui` may only import `models` + `services` (layer rule). Drivers never import ui/app/models/services.
- The result table ALREADY virtualizes: `virtual_row_height = 28.0`, `virtual_buffer = 10` (result_table.rs:198-199). `display_rows_cache` materialized in a `use_effect` (lines 137-153). `updated_cells_set` is an O(1) HashSet built once per render (208-212).
- `--color-error` is UNDEFINED in both themes but referenced in `_toast.scss` (fallback #e74c3c), `_error-boundary.scss` (#e74c3c), and unguarded in `_batch-results.scss`. `--color-focus` EXISTS (line 28 both themes).
- Theme switching = class on root `.app` (`theme-dark`/`theme-light`). SCSS via `grass` build-dep in `app/build.rs` -> `app/assets/app.css` injected via `document::Style`.
- Explorer cache: `EXPLORER_CACHE_TTL = 300s` (app_state/mod.rs:46).

## Wave A - ClickHouse editing reality (VERIFIED)
- `insert_table_row` (mutations.rs:119) has NO UI consumer (only re-export + facade smoke test). UI routes: `insert_empty_row` (result_table.rs:2087, draft-staging) -> `apply_pending_changes` (2186) -> `insert_table_row_with_values` (2236).
- `delete_table_row` CH arm ALREADY EXISTS (mutations.rs:393-428, ALTER TABLE DELETE WHERE via parse_clickhouse_locator). `update_table_cell` CH arm exists (77-115, ALTER TABLE UPDATE). Do NOT re-implement.
- `build_insert_row_sql` (mod.rs:484-508) returns `insert into {qualified} default values` when column_values empty (489-491). CH rejects DEFAULT VALUES. Fix: CH arm of `insert_table_row_with_values` emits `insert into {qualified} () values ()` when empty.
- preview.rs:123 computes `_row_locators` (discarded). lines 159-160 `let editable = None;` + product-policy comment. pk_count slicing: locators from `row[..pk_count]` (143), columns/rows from `row[pk_count..]` (162-177).

## Review verdicts (plan already high-accuracy reviewed, both approved)
- momus: approved after duplicate-todo-9 removal.
- oracle: APPROVED round 3.

## Todo 1 (empty CH insert) - DONE
- CH arm of `insert_table_row_with_values` now delegates to new helper `build_clickhouse_insert_sql` (mutations.rs:194-202).
- Empty column_values -> `insert into {qualified} () values ()` (bypasses build_insert_row_sql's `default values`, which CH rejects). Non-empty -> unchanged build_insert_row_sql path.
- Tests in mutations.rs: `clickhouse_empty_insert_uses_explicit_empty_column_list` (asserts `insert into analytics.events () values ()`, NOT default values) + `clickhouse_non_empty_insert_keeps_build_insert_row_sql_path` (`insert into analytics.events (\`name\`) values ('launch')`).
- `insert_table_row` CH arm untouched (still UnsupportedDriver). cargo test -p query + clippy --all-targets -D warnings clean.

## Wave A - Todo 3: ClickHouse editable previews (query/src/core/preview.rs)

- **Done**: ClickHouse previews now expose `editable = Some(EditableTableContext { source, row_locators })` when the table has a primary key; `None` otherwise. Removed the "read-only for now" product policy marker.
- Key change: binding was `let (response, _row_locators)` discarding locators; now kept and wired into `editable` via `pk_result.as_ref().map(|_| ...)`.
- `source` is passed UNMODIFIED into `EditableTableContext` (schema may be None; mutations re-derive "default" — confirmed in mutations.rs:78-81).
- Locators are built from `row[..pk_count]` while `page.columns`/`page.rows` use `response.meta[pk_count..]` / `row[pk_count..]` — index alignment preserved (query emits `select pk_select, *`).
- **Test seam**: no live CH server in CI. Extracted pure helper `clickhouse_row_locators(pk_columns, &response.data)` in preview.rs; unit tests assert locators use leading PK columns only and tolerate missing/Null PK values. 2 new tests pass; full `cargo test -p query` = 121 passed; clippy clean.
- **Escape note**: `editable` for pk-less CH tables stays `None`; other DB paths untouched.

## Wave A DONE (todos 1-4)
- todo 1: CH empty-insert fixed in `insert_table_row_with_values` via `build_clickhouse_insert_sql` (mutations.rs:194-202) emitting `() values ()` when empty. Tests added (mutations.rs tests).
- todo 2: verified CH delete (393-439) + update (77-115) arms intact, single delete arm. No code change.
- todo 3: preview.rs now `editable = pk_result.as_ref().map(|_| EditableTableContext{source,row_locators})`. Kept `_row_locators` binding. Extracted `clickhouse_row_locators` helper + 2 tests.
- todo 4: UI wiring verified complete (table_cells_editable=page.editable.is_some()&&!read_only_mode at line 241; apply_pending_changes routes to insert_table_row_with_values at 2236 + delete_table_row at 2264; draft helpers untouched; read_only_mode_block_status at actions.rs:95). No UI code change. Non-MergeTree regression test NOT feasible (no CH server in CI, no async/signal harness in ui crate) - documented.
- cargo test -p query: 121 passed. cargo check --workspace: exit 0. clippy clean.

## Wave B DONE (todos 5-7)
- todo 5: added --color-error, --color-error-hover, --focus-ring to both themes; $motion-fast=100ms, $motion-normal=180ms to _tokens.scss. Var audit: all referenced --color-* now defined. app.css compiled (2 error tokens).
- todo 6: refactored focus-ring mixin to use --focus-ring; interactive mixin uses $motion-fast; inputs + tree-nav transition durations tokenized. buttons inherited via @include interactive.
- todo 7: workspace grid/panels/resize migrated to $spacing-*/$radius-*/$motion-*; collapse animation via .workspace--sidebar-hidden; dropzone--active + drop-target affordance (primary outline+halo); resize-handle hover/active. Intentional raw px documented.
- Note: other partials (batch-results, connect-screen, toolbar, result-grid, editor, data-diff, agent-panel) still have 120ms hardcoded - NOT in scope (todo 6 file-scoped to buttons/inputs/tree-nav).
- cargo build -p app: passes.

## Wave C - Todo 10: Settings modal polish (DONE)
- ui/src/layout/settings_modal.rs: split the "Workspace" section into four coherent sub-groups (Defaults / Session and safety / Visible panels by default / AI features) using a new `settings-modal__group` + `__group-title` pair; added `__section-actions` wrapper for reset buttons; aria-pressed + role/aria_label on the segmented theme control; added `aria_disabled` + `__toggle--disabled` modifier to the agent-panel toggle when AI features are off (Dioxus RSX has no `data-*` generic attribute, so disabled state uses a class modifier).
- styles/components/_settings-modal.scss: tokenized all raw px values (6px -> $spacing-sm/xs, 14px -> literal radius, etc.); added hover/focus transitions using `$motion-fast`; segmented control now lives inside a glass pill (border + inset highlight); toggle rows get hover feedback + `:has(input:focus-visible)` focus ring; group titles are uppercase letterspaced labels.
- styles/base/_tokens.scss: added $motion-fast=100ms, $motion-normal=180ms (was missing despite learnings from todo 5).
- styles/themes/_theme-{dark,light}.scss: added --color-error, --color-error-hover, --focus-ring (was missing).
- models/src/settings.rs: new regression test `toggle_single_field_round_trip_preserves_all_persisted_fields` - simulates every modal `set_*` setter (one at a time) by mutating a single field, JSON-round-tripping, and asserting every persisted field is preserved. Added `AppThemePreference` to the test imports and a `ToggleFn` type alias to keep clippy clean.
- Verification: cargo build -p app --features desktop passes. cargo test -p models --lib: 52 passed (was 51). cargo clippy -p models --all-targets -- -D warnings: clean. cargo clippy -p ui: no warnings on settings_modal.rs.
- Persist verification: every modal toggle routes through its `set_*` helper in app_state/mod.rs -> `update_ui_settings` -> `sync_runtime_ui_settings` -> persistence via `app.rs` use_effect watching `APP_UI_SETTINGS` -> `services::save_app_ui_settings_with_secrets`. Theme segmented control: `set_theme_preference` -> updates `theme` -> `sync_runtime_ui_settings` writes `APP_THEME` -> `<div class="app {theme_name}">` re-themes CSS variables.
- Cargo check has pre-existing errors in workspace/mod.rs (keyboard module + request_focus_* not yet wired in another in-progress todo). My files: zero errors, zero clippy warnings.

