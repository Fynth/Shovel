# dbeaver-class-polish - Work Plan

## TL;DR (For humans)

**What you'll get.** Shovel already *is* a functional DBeaver-class database client (Rust + Dioxus 0.7 desktop). This plan takes it from "functional but unpolished" to DBeaver-grade: it closes the one documented feature gap (ClickHouse table-row editing), hardens the design-token system so dark/light theming is consistent, adds pleasant responsive motion and coherent empty/loading/error states, and locks in performance headroom (the result grid already virtualizes — we protect and extend that discipline). Everything lands with regression tests and the CI gates green.

**Why this approach.** The request is open-ended ("build a real DBeaver with beautiful, responsive, optimized UI") on top of an already-built product. I routed it UNCLEAR, so I adopt best-practice defaults and apply them loudly for your veto: polish in place (no ground-up redesign of a working token system), treat ClickHouse editing as in-scope because the repo itself flags it "Not yet", make no silent behavior changes to result limits, and add no new external crates without justification. **Decisions I made for you** — see "Scope OUT" and "Success criteria" for the guardrails; veto any line at the gate.

**What it will NOT do.** No new database drivers. No packaging/CI changes. No visual-identity redesign from scratch (tokens/palette evolve in place). No silent page-size/limit changes. No new persisted settings are added — CH editing is enabled by default (when a table has a primary key) behind the existing read-only mode, with no new toggle. No ACP feature work.

**Effort.** Architecture-scale, 5 workstreams, ~26 implementation todos across `ui`, `query`, `models`, `styles`. Estimate several focused worker sessions.

**Risk.** Medium. The hot files (`result_table.rs`, `workspace/mod.rs`, `mutations.rs`, `query-core/src/lib.rs`) are large and orchestration-heavy. All edits are additive or localized; the CH editing change alters behavior of a currently-read-only path, so it is regression-gated. Dirty-worktree guard: verifiers must reject any plan step that overwrites uncommitted user changes.

---

## Scope

**In scope**
- **CH row editing** (`query/src/core/{preview,mutations}.rs`, `query`/`services` re-exports, driver-clickhouse usage, ui gating via existing `read_only_mode` + `page.editable`).
- **UI/UX polish** — motion/animation, coherent empty/loading/error states, spacing/density, keyboard-shortcut completeness, status-bar/panel micro-polish, DBeaver-grade ergonomics.
- **Performance** — virtual grid correctness + render-budget audit, memoization to stop whole-tree re-renders on signal writes, explorer-cache hardening, query lazy-load boundaries, `use_memo`/`use_resource` where the tree over-reads global signals.
- **Design tokens/theming** — add missing tokens (e.g. `--color-error` referenced in `_toast.scss`/`_batch-results.scss`/`_error-boundary.scss` but undefined in both themes), unify glass/shadow/focus-rings, ensure light/dark parity.
- **Verification/QA** — cargo fmt/clippy/test pass, unit tests for CH mutations + preview editability, final verification wave F1–F4.

**Scope OUT (guardrails, not reductions)**
- No new DB drivers.
- No packaging/CI/workflow changes.
- No new persisted settings unless explicitly added (none added here).
- No behavior change to `default_page_size`, page limits, or result caps.
- No external crate added without justification in the todo that introduces it.
- No user-modified files overwritten (dirty-worktree guard).

---

## Verification strategy

Per-todo QA is agent-executed (no human in the loop): each todo carries happy-path + failure-path scenarios, each with an exact evidence path (command + assertion). Global gates run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Final verification wave (F1–F4) runs in parallel after all todos and every verifier must APPROVE. All verifiers are read-only; none may edit files. The dual high-accuracy review (momus + independent oracle) runs on the complete plan before handoff.

---

## Execution strategy

6 waves, dependency-ordered so CH-editing (the only behavior change) is verified before any polish layer assumes it:

1. **Wave A — ClickHouse row editing** (components C1). Standalone; earliest to fail.
2. **Wave B — Design tokens & theming** (C4). Pure styles; unblocks consistent polish visuals.
3. **Wave C — UI/UX polish & responsiveness** (C2). Depends on B for coherent tokens.
4. **Wave D — Performance optimization** (C3). Independent of B/C visuals; run after A so CH path perf is covered too.
5. **Wave E — Verification & QA hardening** (C5). Final wave + F1–F4.

Each wave is one or more todos; implementation + tests are ONE todo. Every todo lists exhaustive References, Acceptance, happy+failure QA with evidence paths, and a Commit line.

---

## Todos

### Wave A — ClickHouse row editing

- [x] 1. `query/src/core/mutations.rs`: fix the CH empty-insert path in `insert_table_row_with_values` (NOT `insert_table_row` — that fn has no UI consumer; the UI routes through `insert_empty_row` → `apply_pending_changes` at `result_table.rs:2236` → `insert_table_row_with_values`). Today, for a CH draft with all values `None`, `column_values` is empty and `build_insert_row_sql` (mod.rs:489-491) returns `insert into {qualified} default values`, which ClickHouse rejects (no `DEFAULT VALUES` clause). Fix: in the CH arm of `insert_table_row_with_values`, when `column_values.is_empty()`, emit the CH-valid all-default row `insert into {qualified} () values ()` (explicit empty column list) via `ClickHouseDriver.execute_text_query`, bypassing `build_insert_row_sql`. Keep the non-empty CH path (build_insert_row_sql with columns) unchanged. Acceptance: CH empty draft insert emits `insert into {qualified} () values ()` and returns `Ok(())`; non-empty CH insert unchanged. QA happy: unit test asserts the exact generated CH SQL for an empty column list equals `insert into {qualified} () values ()` and that `execute_text_query` is invoked. QA failure: assert the CH empty path never emits `default values` (grep generated SQL). Commit: "query-core: CH empty-row insert uses valid () values () form".
- [x] 2. `query/src/core/mutations.rs`: VERIFY (do not re-implement) the existing CH arms — `delete_table_row` (lines 393–428, `ALTER TABLE {qualified} DELETE WHERE {locator_conditions}` via `parse_clickhouse_locator`) and `update_table_cell` (lines 77–115, `ALTER TABLE UPDATE ... WHERE`). Acceptance: no new duplicate arms added; existing CH delete/update stay intact and wired. QA happy: existing `mutations` tests still pass; grep confirms only ONE CH `delete_table_row` arm. QA failure: clippy dead-code flags a duplicate — assert none. Commit: none (no code change).
- [x] 3. `query/src/core/preview.rs`: for `DatabaseConnection::ClickHouse`, when `pk_result` is `Some`, set `editable: Some(EditableTableContext { source, row_locators })` using the `row_locators` ALREADY computed at lines 123–145 (currently discarded as `_row_locators`) — change the binding to keep them and wire into `editable`; replace `let editable = None;` + "Product policy: ClickHouse table previews are read-only for now" at lines 159–160. When pk is `None`, keep `editable = None`. Note: `source` here is the `TablePreviewSource` function param (schema defaulted to `"default"` at lines 115-118), consistent with what `update_table_cell`/`delete_table_row` re-derive. Acceptance: a CH table with a primary key returns `page.editable = Some(...)` with correct locators; pk-less tables stay `None`. QA happy: unit/integration asserts `page.editable.is_some()` for a PK table. QA failure: assert pk-less → `editable.is_none()`, AND assert the PK columns are excluded from `page.columns`/`rows` but included in locators (the `row[..pk_count]` slicing at lines 162–177 stays consistent with locators built at line 143) — no index mismatch. Commit: "query-core: expose CH editable previews when table has primary key".
- [x] 4. `ui/src/screens/workspace/components/result_table.rs`: VERIFY the editing gate at line 240–241 (`table_cells_editable = page.editable.is_some() && !read_only_mode`) now renders insert/delete affordances enabled for a CH PK-backed preview, and that `apply_pending_changes` (line 2186) routes CH inserts via `services::insert_table_row_with_values` (line 2236; CH arm fixed in todo 1) and deletes via `services::delete_table_row` (line 2264; CH arm already present). The ONLY change needed is todo 3 (preview now returns `editable=Some`). Do NOT modify `insert_empty_row` (line 2087) or `delete_selected_row` (line 2301) — they are draft-staging helpers that do not call the mutation fns directly. Acceptance: CH PK-backed preview renders enabled insert/delete controls; `apply_pending_changes` applies through the CH-capable services; read-only mode still blocks. QA: insert/delete a CH row via toolbar → status reports success. QA failure: read-only mode suppresses via `read_only_mode_block_status` (actions.rs:95); also add a test that a non-MergeTree CH table surfaces the driver error as status text (not a panic). Commit: "ui: enable CH row editing toolbar when preview is editable".

### Wave B — Design tokens & theming

- [x] 5. `styles/base/_tokens.scss` + both `styles/themes/_theme-*.scss`: define canonical `--color-error` (currently undefined — `_toast.scss`, `_batch-results.scss`, `_error-boundary.scss` fall back to hardcoded `#e74c3c`), `--color-error-hover`, `--focus-ring` shadow token, and any missing surface tokens referenced by components. Acceptance: `grep -rn "var(--color-error" styles/` returns definitions in both themes; no component references an undefined `var(--...)` (audit via script over compiled CSS + source grep). QA happy: `cargo build` injects app.css with new tokens; light+dark swap still works. QA failure: grep for each referenced var finds its definition. Commit: "styles: add canonical --color-error and focus tokens to both themes".
- [x] 6. `styles/components/_buttons.scss`, `_inputs.scss`, `_tree-nav.scss`: unify focus-visible ring using the new `--color-focus`/shadow token and `base/_states.scss` mixin `focus-ring`; unify hover/active transitions with one duration token (add `--motion-fast`/`--motion-normal` to tokens if absent). Acceptance: every interactive control has a focus-visible ring; transition durations come from tokens, not hardcoded ms. QA happy: focus a button via keyboard, ring shows; QA failure: missing ring on a control → grep for `focus-visible`/`focus-ring` across these partials. Commit: "styles: unify focus rings + motion duration tokens".
- [x] 7. `styles/layout/_workspace-grid.scss`, `_workspace-panels.scss`: polish panel spacing, collapse animation, drag-drop drop-zone affordance, resize-handle hover/active feedback (tokens only). Acceptance: dock panels use token spacing; drag grip + drop zone have visible states; resize handle has hover/active feedback. QA happy: screenshot/manual grid drag shows drop zone highlight. QA failure: no hardcoded px spacing in these partials (grep). Commit: "styles: polished panel spacing, collapse + drop-zone feedback".

### Wave C — UI/UX polish & responsiveness

- [ ] 8. `ui/src/app_state/mod.rs` + `ui/src/app.rs`: complete keyboard-shortcut coverage. Enumerate the concrete set to add and verify in one handler (existing in `workspace/mod.rs` onkeydown): Ctrl+Return run, Ctrl+E focus editor, Ctrl+Shift+F format, Ctrl+T new tab, Ctrl+W close tab (already present); add and assert Ctrl+Shift+N new tab, Ctrl+F focus/find-in-results, Ctrl+Shift+S save query, Esc close modal/context menu. Acceptance: the listed shortcuts live in one handler; each triggers the same path as the toolbar button. QA happy: for EACH listed shortcut, dispatch the key and assert the expected action fires (agent-driven via Dioxus event). QA failure: a deliberately-unmapped key does nothing and does not crash. Commit: "ui: centralized keyboard shortcut coverage".
- [ ] 9. `ui/src/screens/workspace/components/result_table.rs` + `tabs.rs`: coherent empty/loading/error states (skeleton already exists; unify empty "Query returned no rows" + error states with an icon + retry affordance). Acceptance: empty state shows icon+text+action; error state shows actionable message with retry; loading shows skeleton. QA happy: run a query returning 0 rows → empty state; force an error → error+retry. QA failure: transient error shows retry, not a dead end. Commit: "ui: unify empty/loading/error states in result + editor panes".
- [ ] 10. `ui/src/layout/settings_modal.rs`: polish settings modal sections (grouping, toggle rows, segmented theme control), ensure the new design tokens apply, and confirm every toggle updates `APP_UI_SETTINGS` + persists. Acceptance: all persisted toggles render with token-styled rows; changing them persists (storage file updated). QA happy: toggle a setting, relaunch, assert `app_ui_settings.json` reflects the change. QA failure: toggle a setting, assert the storage file did NOT silently drop the value (no regression) and the modal renders all sections without a broken segmented control. Commit: "ui: polish settings modal + persist consistency".

### Wave D — Performance optimization

- [ ] 11. `ui/src/screens/workspace/components/result_table.rs`: performance budget audit of the virtualized grid. Verify virtualization constants (`virtual_row_height = 28`, `virtual_buffer = 10`), confirm the O(1) lookup structures (already present, `display_rows_cache` materialization effect), and convert any per-cell allocations or O(n) scans to cached structures. Add `use_memo` where the grid recomputes `display_rows_cache` (already in a `use_effect`) or headers recompute on every render. Acceptance: no O(n) per-cell scan; `use_memo`/cache added where a signal re-render would rebuild rows; DOM row count stays `~viewport + 20`. QA happy: render 10k rows, DOM stays bounded (measured via agent instrumenting row count); scroll is smooth. QA failure: row count grows with dataset → fail (would have defeated virtualization). Commit: "ui: harden result-grid virtualization + memoization".
- [ ] 12. `ui/src/app_state/mod.rs` + consumers: reduce whole-tree re-render churn — verify `APP_UI_SETTINGS`/`APP_SHOW_*` global signals only notify subscribers, add `use_memo` splits in heavy panels (explorer, agent panel, tabs) so a single global write doesn't rebuild the whole workspace. Acceptance: a panel-visibility toggle re-renders only affected subtree, not the entire `.app`. QA happy: instrument a render counter on the root `.app` element; toggle a panel-visibility signal and assert the root render counter delta is 0 (only the target subtree re-renders). QA failure: if the root counter increments on a single toggle, flag and fix by scoping consumers. Commit: "ui: scope global-signal consumers to avoid whole-tree re-renders".
- [ ] 13. `ui/src/app_state/mod.rs`: explorer cache — confirm 5-min TTL cache (`EXPLORER_CACHE_TTL`) is honored by all reads; add staleness/eviction check. Acceptance: cached explorer sections served within TTL, evicted after; no unbounded growth. QA happy: load tree twice, second is cache-hit (no reload). QA failure: cache eviction path on expiry doesn't panic. Commit: "ui: harden explorer cache TTL/eviction".

### Wave E — Verification & QA hardening

- [ ] 14. Tests: add unit tests for the CH mutations — the empty-insert CH path in `insert_table_row_with_values` (todo 1, assert exact `() values ()` SQL) and the preview editability (todo 3). Assert pk/read-only gating and the non-MergeTree error-surfacing (todo 4). Commit: "test: CH row-edit mutations + preview editability".
- [ ] 15. `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` — full CI gate green. Fix any warnings introduced. QA: the three commands exit 0. Commit: "chore: fmt/clippy pass".

---

## Final verification wave

Runs in parallel after ALL todos; every verifier must APPROVE.

- [ ] F1. **Plan compliance audit** — verify every todo's acceptance criteria met in code, no scope-out item was added, no user files overwritten (dirty-worktree guard). Verifier: read-only Oracle, **distinct from the todo executor** (identity recorded in the wave log).
- [ ] F2. **Code quality review** — clippy `-D warnings`, tests green, no dead code, layer rules respected (`ui` only imports `models`+`services`; drivers don't import ui/app). Verifier: read-only Oracle, **distinct from the todo executor** (identity recorded in the wave log).
- [ ] F3. **Real manual QA** — launch `cargo run -p app --features desktop`; verify CH row editing, grid virtualization smoothness, theme consistency light/dark, keyboard shortcuts. Evidence: session logs/screenshots. Verifier: Oracle with hands-on run.
- [ ] F4. **Scope fidelity** — confirm Scope OUT was not violated (no new driver, no CI changes, no page-size behavior change). Verifier: read-only Oracle.

---

## Commit strategy

- One logical commit per todo, message prefixed with `todo N: ` + descriptive subject (e.g. `todo 1: query-core: support CH insert+delete`). 
- Feature (impl + test) in ONE commit (per-todo pairing).
- `git add` only the files the todo touches; never stage unrelated modifications or user files outside the todo's scope.
- CH behavior change commit is explicit + regression-gated (todo 3 before polish waves).
- After the plan's final wave, aggregate into the worker's feature branch as the exec completes.

---

## Success criteria

- A CH connection with a PK-backed table allows editing rows (edit cells, insert, delete) from the result grid; pk-less CH tables remain read-only; read-only mode still blocks edits.
- ClickHouse table previews expose `editable` only when a primary key exists.
- Both dark and light themes are coherent: no undefined `var(--color-*)` references, consistent focus rings, motion duration from a single token.
- The workspace feels responsive: panel toggles re-render only the affected subtree; result grid DOM stays bounded at any dataset size (virtualization preserved); explorer cache serves within TTL.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all green.
- All four F1–F4 verifiers APPROVE.
