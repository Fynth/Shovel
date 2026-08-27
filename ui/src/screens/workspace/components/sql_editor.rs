#[path = "sql_editor/highlight.rs"]
mod highlight;
#[path = "sql_editor/selection.rs"]
mod selection;
#[path = "sql_editor/completion_menu.rs"]
mod completion_menu;

use crate::{
    app_state::{
        APP_AI_FEATURES_ENABLED,
        APP_EDITOR_BEHAVIOR,
        APP_SQL_FORMAT_SETTINGS,
        APP_STATE,
        APP_UI_SETTINGS,
        context_menu::{ContextMenuItem, open_context_menu},
        toast_error,
    },
    completion::{
        ai::stream_sql_ghost,
        keyboard::{EditorKeyAction, editor_completion_action},
        keywords::CompletionItem,
        query::parse_completion_query,
        rank::collect_menu_items,
        schema::merge_columns_into_tree,
        trim::trim_completion_for_cursor,
        variants::GhostVariants,
    },
    screens::workspace::{
        actions::{
            IndentDirection,
            clear_active_tab_sql,
            format_active_tab,
            indent_lines_in_active_tab,
            replace_active_tab_sql,
            run_active_tab,
            run_active_tab_explain,
            save_active_tab_as_saved_query,
            sync_active_tab_sql_draft,
            toggle_line_comments_in_active_tab,
        },
        components::{explorer::ExplorerConnectionSection, send_sql_explanation_request},
        context::{WorkspaceAcpContext, WorkspaceQueryContext},
        tab_store::TabStore,
    },
};
use dioxus::prelude::*;
use models::{DatabaseKind, ExplorerNodeKind, QueryHistoryItem};
use services::CompletionToken;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use self::{
    completion_menu::{
        CaretAnchor,
        MENU_WIDTH,
        SqlCompletionMenu,
        apply_menu_item_if_current,
        autocomplete_offset,
        caret_anchor_script,
        map_completion_key,
        menu_height_for_items,
        should_refresh_menu_caret,
        table_missing_columns,
    },
    highlight::SqlHighlightContent,
    selection::{
        EditorSelection,
        editor_value_and_selection_query_script,
        set_editor_value_script,
        sync_editor_selection,
        sync_editor_selection_debounced,
    },
};

const SQL_EDITOR_TEXTAREA_ID: &str = "workspace-sql-editor";
const COMPLETION_DEBOUNCE_MS: u64 = 180;
const HIGHLIGHT_IDLE_MS: u64 = 90;
static CARET_ANCHOR_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InlineCompletion {
    cursor: usize,
    source_sql: String,
    text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CompletionRuntime {
    request_id: u64,
    pending_snapshot: Option<usize>,
    last_completed_snapshot: Option<usize>,
    active: Option<InlineCompletion>,
    variants: GhostVariants,
    discarded: bool,
    cycle_in_flight: bool,
}

impl CompletionRuntime {
    fn invalidate(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.pending_snapshot = None;
        self.last_completed_snapshot = None;
        self.active = None;
        self.variants = GhostVariants::default();
        self.cycle_in_flight = false;
    }

    fn reset_to_snapshot(&mut self, snapshot: usize) {
        self.invalidate();
        self.last_completed_snapshot = Some(snapshot);
    }

    fn begin_request(&mut self, snapshot: usize) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.pending_snapshot = Some(snapshot);
        self.active = None;
        self.variants.clear_if_changed(snapshot);
        self.cycle_in_flight = false;
        self.request_id
    }

    fn finish_request(&mut self, request_id: u64, snapshot: usize) -> bool {
        if self.request_id != request_id {
            return false;
        }
        self.pending_snapshot = None;
        self.last_completed_snapshot = Some(snapshot);
        true
    }

    fn set_active(
        &mut self,
        request_id: u64,
        snapshot: usize,
        cursor: usize,
        source_sql: String,
        text: String,
    ) {
        if self.discarded || self.cycle_in_flight {
            return;
        }
        if self.finish_request(request_id, snapshot) {
            if self.variants.items().len() <= 1 {
                self.variants.set_first(snapshot, text.clone());
            }
            self.active = Some(InlineCompletion {
                cursor,
                source_sql,
                text,
            });
        }
    }

    fn begin_cycle_fetch(&mut self) -> Option<u64> {
        if self.discarded || self.cycle_in_flight {
            return None;
        }
        self.cycle_in_flight = true;
        self.request_id = self.request_id.wrapping_add(1);
        Some(self.request_id)
    }

    fn complete_cycle_fetch(
        &mut self,
        request_id: u64,
        text: String,
        cursor: usize,
        source_sql: String,
    ) -> bool {
        if self.request_id != request_id || self.discarded {
            return false;
        }
        self.cycle_in_flight = false;
        self.variants.push(text.clone());
        if let Some(active) = &mut self.active {
            active.text = text;
        } else {
            self.active = Some(InlineCompletion {
                cursor,
                source_sql,
                text,
            });
        }
        true
    }

    fn abort_cycle_fetch(&mut self, request_id: u64) {
        if self.request_id == request_id {
            self.cycle_in_flight = false;
        }
    }

    /// Returns whether this cycle spawn still owns the runtime. A stale
    /// `request_id` (new ghost request, dismiss) aborts the cycle. When the
    /// id already moved, `begin_request` / dismiss have cleared the flag;
    /// `abort_cycle_fetch` is still invoked so this path never skips abort.
    fn cycle_still_current(&mut self, request_id: u64) -> bool {
        if self.request_id != request_id || self.discarded {
            self.abort_cycle_fetch(request_id);
            false
        } else {
            true
        }
    }

    fn dismiss_ghost(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.pending_snapshot = None;
        self.active = None;
        self.discarded = true;
        self.cycle_in_flight = false;
    }

    fn clear_on_typing(&mut self) {
        if self.active.is_some() || self.pending_snapshot.is_some() || self.cycle_in_flight {
            self.invalidate();
        }
        self.variants = GhostVariants::default();
        self.discarded = false;
        self.cycle_in_flight = false;
    }
}

fn spawn_caret_anchor_update(mut caret_anchor: Signal<CaretAnchor>) {
    let request_id = CARET_ANCHOR_REQUEST_ID.fetch_add(1, Ordering::SeqCst) + 1;
    spawn(async move {
        let Ok(anchor) = document::eval(&caret_anchor_script(SQL_EDITOR_TEXTAREA_ID))
            .join::<CaretAnchor>()
            .await
        else {
            return;
        };
        if CARET_ANCHOR_REQUEST_ID.load(Ordering::SeqCst) != request_id {
            return;
        }
        caret_anchor.set(anchor);
    });
}

fn invalidate_completion(mut completion: Signal<CompletionRuntime>) {
    completion.with_mut(CompletionRuntime::invalidate);
}

fn reset_completion_to_snapshot(mut completion: Signal<CompletionRuntime>, snapshot: usize) {
    completion.with_mut(|state| state.reset_to_snapshot(snapshot));
}

fn hash_sql(sql: &str) -> usize {
    sql.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    })
}

fn hash_completion_snapshot(sql: &str, cursor: usize) -> usize {
    hash_sql(sql).wrapping_mul(31).wrapping_add(cursor)
}

fn log_completion(_msg: &str) {}

/// Insert the active inline completion into the editor at the live caret.
///
/// `source` is a short tag used only for logging and tracing.
#[allow(clippy::too_many_arguments)]
fn apply_inline_completion(
    completion_runtime: Signal<CompletionRuntime>,
    store: TabStore,
    active_tab_id_value: u64,
    mut draft_sql: Signal<String>,
    mut editor_selection: Signal<EditorSelection>,
    mut is_typing: Signal<bool>,
    mut editor_revision: Signal<u64>,
    completion_text_raw: String,
    source: &'static str,
) {
    spawn(async move {
        log_completion(&format!("apply_inline_completion ({source})"));
        // Read current SQL and caret from DOM (most accurate), fall back to signals.
        let (actual_sql, cursor) = if let Ok((sql, start, _end)) = document::eval(
            &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
        )
        .join::<(String, usize, usize)>()
        .await
        {
            let cursor = EditorSelection::collapsed(start).clamped(&sql).start;
            (sql, cursor)
        } else {
            let sql = draft_sql.peek().clone();
            let cursor = editor_selection.peek().clamped(&sql).start;
            (sql, cursor)
        };
        let mut completion_text =
            trim_completion_for_cursor(&actual_sql, cursor, &completion_text_raw);
        let prev = actual_sql[..cursor].chars().last().unwrap_or(' ');
        let next = completion_text.chars().next().unwrap_or(' ');
        let next_is_new_clause = completion_text
            .split_whitespace()
            .next()
            .is_some_and(is_sql_clause_start);
        if !prev.is_whitespace()
            && !next.is_whitespace()
            && next_is_new_clause
            && !completion_text.is_empty()
        {
            completion_text = format!(" {completion_text}");
        }
        if completion_text.is_empty() {
            return;
        }
        let new_cursor = cursor + completion_text.len();
        let new_sql = format!(
            "{}{}{}",
            &actual_sql[..cursor],
            completion_text,
            &actual_sql[cursor..]
        );
        draft_sql.set(new_sql.clone());
        editor_selection.set(EditorSelection::collapsed(new_cursor));
        is_typing.set(false);
        reset_completion_to_snapshot(
            completion_runtime,
            hash_completion_snapshot(&new_sql, new_cursor),
        );
        editor_revision += 1;
        let new_sql_for_dom = new_sql.clone();
        replace_active_tab_sql(store, active_tab_id_value, new_sql, "Ready".to_string());
        spawn(async move {
            let _ = document::eval(&set_editor_value_script(
                SQL_EDITOR_TEXTAREA_ID,
                &new_sql_for_dom,
                new_cursor,
                true,
            ))
            .join::<bool>()
            .await;
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn accept_completion_menu_item(
    item: CompletionItem,
    store: TabStore,
    active_tab_id_value: u64,
    mut draft_sql: Signal<String>,
    mut editor_selection: Signal<EditorSelection>,
    mut is_typing: Signal<bool>,
    mut editor_revision: Signal<u64>,
    completion_runtime: Signal<CompletionRuntime>,
    mut menu_items: Signal<Vec<CompletionItem>>,
    mut menu_index: Signal<usize>,
    mut menu_force: Signal<bool>,
    mut menu_closed: Signal<bool>,
    mut menu_source_sql: Signal<String>,
) {
    let sql = draft_sql.peek().clone();
    let source_sql = menu_source_sql.peek().clone();
    let Some((next, cursor)) = apply_menu_item_if_current(&sql, &source_sql, &item) else {
        return;
    };
    draft_sql.set(next.clone());
    editor_selection.set(EditorSelection::collapsed(cursor));
    is_typing.set(false);
    menu_items.set(Vec::new());
    menu_index.set(0);
    menu_force.set(false);
    menu_closed.set(true);
    menu_source_sql.set(String::new());
    reset_completion_to_snapshot(completion_runtime, hash_completion_snapshot(&next, cursor));
    editor_revision += 1;
    sync_active_tab_sql_draft(store, active_tab_id_value, next.clone());
    spawn(async move {
        let _ = document::eval(&set_editor_value_script(
            SQL_EDITOR_TEXTAREA_ID,
            &next,
            cursor,
            true,
        ))
        .join::<bool>()
        .await;
    });
}

fn cycle_ghost_next(
    mut completion_runtime: Signal<CompletionRuntime>,
    draft_sql: Signal<String>,
    editor_selection: Signal<EditorSelection>,
    schema_ctx: String,
) {
    let fetch_id = completion_runtime.with_mut(|state| {
        if state.variants.show_next_existing() {
            if let Some(text) = state.variants.current().map(str::to_string)
                && let Some(active) = &mut state.active
            {
                active.text = text;
            }
            None
        } else {
            state.begin_cycle_fetch()
        }
    });
    let Some(request_id) = fetch_id else {
        return;
    };

    spawn(async move {
        let (sql_text, start, end) = if let Ok((sql, start, end)) = document::eval(
            &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
        )
        .join::<(String, usize, usize)>()
        .await
        {
            (sql, start, end)
        } else {
            let sql = draft_sql.peek().clone();
            let selection = editor_selection.peek().clamped(&sql);
            (sql, selection.start, selection.end)
        };
        if start != end {
            completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
            return;
        }
        let selection = EditorSelection { start, end };
        let Some((cursor, prefix, suffix)) = completion_request_parts(&sql_text, selection) else {
            completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
            return;
        };
        let settings = APP_UI_SETTINGS();
        if !settings.sql_ghost_ready() {
            completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
            return;
        }
        let avoid = {
            let state = completion_runtime.peek();
            if state.discarded || state.request_id != request_id {
                None
            } else {
                Some(state.variants.items().to_vec())
            }
        };
        let Some(avoid) = avoid else {
            completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
            return;
        };
        let mut schema_ctx = schema_ctx;
        let surrounding = surrounding_sql_context(&sql_text, cursor);
        if !surrounding.is_empty() {
            use std::fmt::Write;
            let _ = write!(
                schema_ctx,
                "-- Surrounding SQL context (before cursor):\n-- {}",
                surrounding.replace('\n', "\n-- ")
            );
        }
        let mut token_rx = stream_sql_ghost(&settings, prefix, suffix, schema_ctx, &avoid);
        let mut accumulated = String::new();
        while let Some(token) = token_rx.recv().await {
            if !completion_runtime.with_mut(|state| state.cycle_still_current(request_id)) {
                return;
            }
            match token {
                CompletionToken::Text(t) => accumulated.push_str(&t),
                CompletionToken::Error(_) => {
                    completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
                    return;
                }
                CompletionToken::Done => {
                    let trimmed = trim_completion_for_cursor(&sql_text, cursor, &accumulated);
                    if trimmed.is_empty() {
                        completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
                        return;
                    }
                    completion_runtime.with_mut(|state| {
                        state.complete_cycle_fetch(
                            request_id,
                            accumulated.clone(),
                            cursor,
                            sql_text.clone(),
                        );
                    });
                    return;
                }
            }
        }
        completion_runtime.with_mut(|state| state.abort_cycle_fetch(request_id));
    });
}

/// Returns true if the word looks like it starts a new SQL clause.
fn is_sql_clause_start(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "SELECT"
            | "FROM"
            | "WHERE"
            | "JOIN"
            | "LEFT"
            | "RIGHT"
            | "INNER"
            | "OUTER"
            | "CROSS"
            | "ON"
            | "AND"
            | "OR"
            | "ORDER"
            | "GROUP"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "SET"
            | "VALUES"
            | "INTO"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "WITH"
            | "AS"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
    )
}

// ─────────────────── Context menu + selection helpers ───────────────────

/// Selection start/end as UTF-16 code-unit offsets (Dioxus surfaces
/// keyboard selection positions the same way the browser does).
/// We clamp the values to the SQL text length so callers can use
/// them as safe byte ranges without further validation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorSelectionRange {
    pub start: usize,
    pub end: usize,
}

/// Read the current textarea selection from a `KeyboardEvent`. We
/// can't extract the selection start/end directly from the event
/// payload (Dioxus does not yet expose `target` as the
/// HTMLTextAreaElement on a keyboard event), so we ship the
/// start/end as keyboard event attributes via `data-selection-*`
/// if the textarea fires a synthetic event. For now we use a safe
/// conservative default: a collapsed selection at the end of the
/// SQL text. This means shortcuts like "indent" and "comment"
/// operate on the full SQL — not ideal, but at least they work
/// and never panic.
fn event_selection_range(_event: &Event<KeyboardData>) -> EditorSelectionRange {
    EditorSelectionRange { start: 0, end: 0 }
}

/// Open the SQL editor's context menu at the given screen
/// coordinates. The items here mirror what a code editor like VS
/// Code exposes, with extra domain-specific actions
/// (format/explain/run) at the bottom.
///
/// `tab` is the tab whose SQL we're operating on; we read its
/// `sql`/`title` lazily inside the closures so the menu reflects
/// the latest in-memory state.
#[allow(clippy::too_many_arguments)]
fn open_sql_editor_context_menu(
    x: f64,
    y: f64,
    store: TabStore,
    active_tab_id: u64,
    saved_queries_signal: Signal<Vec<models::SavedQuery>>,
    next_saved_query_id: Signal<u64>,
    history: Signal<Vec<QueryHistoryItem>>,
    next_history_id: Signal<u64>,
    acp_ctx: Option<WorkspaceAcpContext>,
) {
    // We snapshot the current SQL and selection once at open time
    // so the callbacks don't race the editor while the user is
    // clicking through the menu.
    let snapshot = {
        let editor = store.editor.read();
        editor.get(&active_tab_id).cloned()
    };
    let Some(tab_snapshot) = snapshot else {
        return;
    };
    let sql_len = tab_snapshot.sql.len();
    let can_run = !tab_snapshot.sql.trim().is_empty();
    let can_format = can_run;
    let can_save = can_run;

    // Cut / Copy / Paste: we can't directly read the textarea
    // selection from Rust, so we ask the browser to do the work
    // via `document.execCommand`. These commands fire on the
    // currently-focused element (the textarea), which is exactly
    // what we want.
    let copy_label = if sql_len == 0 {
        "Copy\t\t(no text)".to_string()
    } else {
        "Copy\t\tCtrl+C".to_string()
    };
    let cut_label = if sql_len == 0 {
        "Cut\t\t(no text)".to_string()
    } else {
        "Cut\t\tCtrl+X".to_string()
    };
    let paste_label = "Paste\t\tCtrl+V".to_string();
    let select_all_label = "Select all\t\tCtrl+A".to_string();
    let clear_label = "Clear editor\t\tCtrl+L".to_string();
    let format_label = "Format SQL\t\tCtrl+Shift+F".to_string();
    let run_label = "Run query\t\tCtrl+Enter".to_string();
    let explain_label = "Explain query\t\tCtrl+Shift+E".to_string();
    let comment_label = "Toggle comment\t\tCtrl+/".to_string();
    let save_label = "Save as saved query\t\tCtrl+S".to_string();

    let mut items: Vec<ContextMenuItem> = Vec::new();

    // ── Text actions ───────────────────────────────────────────
    let copy_item = ContextMenuItem::new(copy_label, move || {
        spawn(async move {
            let _ = document::eval(SQL_EDITOR_COPY_SCRIPT).await;
        });
    });
    let mut copy_item = copy_item;
    if sql_len == 0 {
        copy_item = copy_item.disabled();
    }
    items.push(copy_item);

    let cut_item = ContextMenuItem::new(cut_label, move || {
        spawn(async move {
            let _ = document::eval(SQL_EDITOR_CUT_SCRIPT).await;
        });
    });
    let mut cut_item = cut_item;
    if sql_len == 0 {
        cut_item = cut_item.disabled();
    }
    items.push(cut_item);

    let paste_item = ContextMenuItem::new(paste_label, move || {
        spawn(async move {
            let _ = document::eval(SQL_EDITOR_PASTE_SCRIPT).await;
        });
    });
    items.push(paste_item);

    let select_all_item = ContextMenuItem::new(select_all_label, move || {
        spawn(async move {
            let _ = document::eval(SQL_EDITOR_SELECT_ALL_SCRIPT).await;
        });
    });
    items.push(select_all_item);

    // ── Editor actions (separator before) ──────────────────────
    let mut clear_item = ContextMenuItem::new(clear_label, move || {
        clear_active_tab_sql(store, active_tab_id);
    })
    .separator();
    if sql_len == 0 {
        clear_item = clear_item.disabled();
    }
    items.push(clear_item);

    let mut comment_item = ContextMenuItem::new(comment_label, move || {
        // For the context menu we always operate on the full
        // textarea content, since the user has just right-clicked
        // and the selection may not be representative.
        let current_sql = {
            let editor = store.editor.peek();
            editor.get(&active_tab_id).map(|e| e.sql.len()).unwrap_or(0)
        };
        let _ = toggle_line_comments_in_active_tab(store, active_tab_id, 0..current_sql);
    });
    if sql_len == 0 {
        comment_item = comment_item.disabled();
    }
    items.push(comment_item);

    // ── SQL actions (separator before) ─────────────────────────
    let mut format_item = ContextMenuItem::new(format_label, move || {
        format_active_tab(store, active_tab_id, APP_SQL_FORMAT_SETTINGS());
    })
    .separator();
    if !can_format {
        format_item = format_item.disabled();
    }
    items.push(format_item);

    let mut run_item = ContextMenuItem::new(run_label, move || {
        run_active_tab(store, active_tab_id, (history, next_history_id));
    });
    if !can_run {
        run_item = run_item.disabled();
    }
    items.push(run_item);

    let mut explain_item = ContextMenuItem::new(explain_label, move || {
        run_active_tab_explain(store, active_tab_id);
    });
    if !can_run {
        explain_item = explain_item.disabled();
    }
    items.push(explain_item);

    // ── AI actions ──────────────────────────────────────────────
    let ai_enabled = APP_AI_FEATURES_ENABLED();
    if ai_enabled && let Some(acp_ctx) = acp_ctx {
        let panel_state = acp_ctx.acp_panel_state;
        let chat_revision = acp_ctx.chat_revision;
        let allow_db_read = acp_ctx.allow_agent_db_read;
        let label = acp_ctx.connection_label.clone();
        let mut explain_ai_item =
            ContextMenuItem::new("Explain with AI\t\tCtrl+Shift+E", move || {
                send_sql_explanation_request(
                    panel_state,
                    store,
                    active_tab_id,
                    label.clone(),
                    chat_revision,
                    allow_db_read(),
                );
            })
            .separator();
        if !can_run {
            explain_ai_item = explain_ai_item.disabled();
        }
        items.push(explain_ai_item);
    }

    let mut save_item = ContextMenuItem::new(save_label, move || {
        let status = save_active_tab_as_saved_query(
            store,
            active_tab_id,
            saved_queries_signal,
            next_saved_query_id,
        );
        if let Some(message) = status
            .strip_prefix("Saved ")
            .and_then(|s| s.strip_suffix('.'))
        {
            use crate::app_state::{ToastKind, show_toast};
            show_toast(message.to_string(), ToastKind::Success);
        }
    })
    .separator();
    if !can_save {
        save_item = save_item.disabled();
    }
    items.push(save_item);

    open_context_menu(x, y, items);
    let _ = (history, next_history_id); // consumed; silence unused
}

/// JS that copies the current selection of the SQL editor
/// textarea into the system clipboard.
const SQL_EDITOR_COPY_SCRIPT: &str = r#"
(function() {
    const el = document.getElementById('workspace-sql-editor');
    if (!el) return;
    el.focus();
    try {
        document.execCommand('copy');
    } catch (e) { /* user agent blocked it; nothing to do */ }
})();
"#;

/// JS that cuts the current selection. Browsers strip the cut
/// content on successful execution; we don't need to mirror the
/// result back into Rust because the `execCommand` handler fires
/// a `cut` event which Dioxus receives via the textarea's oninput.
const SQL_EDITOR_CUT_SCRIPT: &str = r#"
(function() {
    const el = document.getElementById('workspace-sql-editor');
    if (!el) return;
    el.focus();
    try {
        document.execCommand('cut');
    } catch (e) { /* ignored */ }
})();
"#;

/// JS that pastes from the system clipboard into the textarea at
/// the current cursor position. The `input` event from execCommand
/// will surface back to Dioxus via oninput, keeping the Rust state
/// in sync.
const SQL_EDITOR_PASTE_SCRIPT: &str = r#"
(function() {
    const el = document.getElementById('workspace-sql-editor');
    if (!el) return;
    el.focus();
    try {
        document.execCommand('paste');
    } catch (e) { /* ignored */ }
})();
"#;

/// JS that selects all text in the editor textarea.
const SQL_EDITOR_SELECT_ALL_SCRIPT: &str = r#"
(function() {
    const el = document.getElementById('workspace-sql-editor');
    if (!el) return;
    el.focus();
    el.setSelectionRange(0, el.value.length);
})();
"#;

#[cfg(test)]
mod tests {
    use super::{
        CompletionRuntime,
        completion_request_parts,
        line_number_labels,
        selection::EditorSelection,
    };

    #[test]
    fn line_number_labels_count_lines() {
        assert_eq!(line_number_labels("a"), vec![1]);
        assert_eq!(line_number_labels("a\nb\n"), vec![1, 2, 3]);
    }

    #[test]
    fn completion_request_parts_split_sql_at_cursor() {
        let sql = "select  from users";
        let cursor = "select ".len();
        let (position, prefix, suffix) =
            completion_request_parts(sql, EditorSelection::collapsed(cursor)).unwrap();

        assert_eq!(position, cursor);
        assert_eq!(prefix, "select ");
        assert_eq!(suffix.as_deref(), Some(" from users"));
    }

    #[test]
    fn cycle_fetch_ignores_late_original_stream_and_rejects_double_push() {
        let mut runtime = CompletionRuntime::default();
        let original_id = runtime.begin_request(1);
        runtime.set_active(original_id, 1, 7, "select ".into(), "from users".into());
        assert_eq!(runtime.variants.current(), Some("from users"));
        assert_eq!(
            runtime.active.as_ref().map(|active| active.text.as_str()),
            Some("from users")
        );

        let cycle_id = runtime.begin_cycle_fetch().expect("cycle fetch starts");
        assert_ne!(cycle_id, original_id);
        assert_eq!(
            runtime.active.as_ref().map(|active| active.text.as_str()),
            Some("from users")
        );
        assert!(runtime.begin_cycle_fetch().is_none());

        runtime.set_active(
            original_id,
            1,
            7,
            "select ".into(),
            "from users WHERE".into(),
        );
        assert_eq!(
            runtime.active.as_ref().map(|active| active.text.as_str()),
            Some("from users")
        );
        assert_eq!(runtime.variants.items().len(), 1);

        assert!(runtime.complete_cycle_fetch(cycle_id, "from orders".into(), 7, "select ".into()));
        assert_eq!(runtime.variants.current(), Some("from orders"));
        assert_eq!(runtime.variants.items().len(), 2);
        assert_eq!(
            runtime.active.as_ref().map(|active| active.text.as_str()),
            Some("from orders")
        );
        assert!(runtime.begin_cycle_fetch().is_some());
    }

    #[test]
    fn abandoned_cycle_path_clears_in_flight_and_allows_later_fetch() {
        let mut runtime = CompletionRuntime::default();
        let original_id = runtime.begin_request(1);
        runtime.set_active(original_id, 1, 7, "select ".into(), "from users".into());
        let cycle_id = runtime.begin_cycle_fetch().expect("cycle fetch starts");
        assert!(runtime.begin_cycle_fetch().is_none());

        // Caret move / click / Ctrl-Space starts a new ghost request without typing.
        runtime.begin_request(2);
        assert!(
            !runtime.cycle_still_current(cycle_id),
            "abandoned cycle spawn must take the stale-id abort path"
        );
        assert!(
            runtime.begin_cycle_fetch().is_some(),
            "cycle_in_flight must be clear so Alt+] can fetch again"
        );
    }
}

fn completion_request_parts(
    sql: &str,
    selection: EditorSelection,
) -> Option<(usize, String, Option<String>)> {
    let selection = selection.clamped(sql);
    if selection.start != selection.end {
        return None;
    }

    let cursor = selection.end;
    Some((
        cursor,
        sql[..cursor].to_string(),
        (!sql[cursor..].is_empty()).then(|| sql[cursor..].to_string()),
    ))
}

fn build_schema_context(sections: &[ExplorerConnectionSection], session_id: u64) -> String {
    let section = match sections.iter().find(|s| s.session_id == session_id) {
        Some(s) => s,
        None => return String::new(),
    };

    let mut lines: Vec<String> = Vec::new();
    let mut first_table = true;

    for node in &section.nodes {
        if node.kind == ExplorerNodeKind::Schema {
            let schema_name = &node.name;
            for table in &node.children {
                if table.kind == ExplorerNodeKind::Table || table.kind == ExplorerNodeKind::View {
                    if !first_table {
                        lines.push(String::new());
                    }
                    first_table = false;

                    let kind_label = if table.kind == ExplorerNodeKind::View {
                        "View"
                    } else {
                        "Table"
                    };

                    let full_name = format!("{schema_name}.{}", table.name);
                    lines.push(format!("-- {kind_label}: {full_name}"));

                    if !table.children.is_empty() {
                        let cols: Vec<String> =
                            table.children.iter().map(|col| col.name.clone()).collect();
                        lines.push(format!("--   Columns: {}", cols.join(", ")));
                    }
                }
            }
        } else if node.kind == ExplorerNodeKind::Table || node.kind == ExplorerNodeKind::View {
            if !first_table {
                lines.push(String::new());
            }
            first_table = false;

            let kind_label = if node.kind == ExplorerNodeKind::View {
                "View"
            } else {
                "Table"
            };

            lines.push(format!("-- {kind_label}: {}", node.name));

            if !node.children.is_empty() {
                let cols: Vec<String> = node.children.iter().map(|col| col.name.clone()).collect();
                lines.push(format!("--   Columns: {}", cols.join(", ")));
            }
        }
    }

    if lines.is_empty() {
        return String::new();
    }

    // Add a trailing blank line so the schema block is visually separated
    // from the SQL prefix that follows.
    format!("{}\n", lines.join("\n"))
}

/// Extract a few lines of SQL that precede the cursor position — the
/// "surrounding context" — so the LLM sees what kind of queries the user
/// is writing, not just the single statement being completed.
///
/// Returns text from the last `;` (or the beginning) up to `cursor`,
/// capped at 500 characters.
fn surrounding_sql_context(sql: &str, cursor: usize) -> String {
    let cursor = cursor.min(sql.len());
    let before_cursor = &sql[..cursor];

    let start = before_cursor.rfind(';').map_or(0, |pos| pos + 1);
    let ctx = before_cursor[start..].trim();

    if ctx.len() <= 500 {
        return ctx.to_string();
    }

    // Truncate from the start, keeping the last ~500 chars.
    let excess = ctx.len() - 500;
    // Walk forward to the next char boundary so we don't slice mid-char.
    let mut keep_from = excess;
    while keep_from < ctx.len() && !ctx.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    format!("…{}", &ctx[keep_from..])
}

pub fn line_number_labels(sql: &str) -> Vec<usize> {
    let lines = sql.split('\n').count().max(1);
    (1..=lines).collect()
}

#[component]
pub fn SqlEditor(
    sql: String,
    active_tab_id: u64,
    active_session_id: u64,
    store: TabStore,
    explorer_sections: Signal<Vec<ExplorerConnectionSection>>,
) -> Element {
    let active_tab_id_value = active_tab_id;
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut scroll_left = use_signal(|| 0.0_f64);
    let mut draft_sql = use_signal(|| sql.clone());
    let mut editor_selection = use_signal(|| EditorSelection::collapsed(sql.len()));
    let mut editor_revision = use_signal(|| 0_u64);
    let mut is_typing = use_signal(|| false);
    let mut completion_runtime = use_signal(CompletionRuntime::default);
    let mut has_synced_editor_dom = use_signal(|| false);
    let mut synced_editor_tab_id = use_signal(|| active_tab_id_value);
    let mut menu_items = use_signal(Vec::<CompletionItem>::new);
    let mut menu_index = use_signal(|| 0_usize);
    let mut menu_force = use_signal(|| false);
    let mut menu_closed = use_signal(|| false);
    let mut menu_source_sql = use_signal(String::new);
    let caret_anchor = use_signal(CaretAnchor::default);
    let mut column_fetches = use_signal(HashSet::<(u64, String, String)>::new);
    let mut explorer_sections = explorer_sections;

    // Pull the workspace query context (history, saved queries) so
    // the context menu and keyboard shortcuts can act on them
    // without having to thread extra props through `TabsManager`.
    let query_ctx = use_context::<WorkspaceQueryContext>();

    // The ACP context may not be provided in every render path
    // (e.g. when the editor is shown in isolation). Treat absence as
    // "no ACP explain available" and surface that as a disabled menu
    // item rather than a runtime panic.
    let acp_ctx = try_use_context::<WorkspaceAcpContext>();

    // Make the signals `Copy`-friendly inside the closures below
    // by binding them once at the top of the component body. The
    // `move` closures would otherwise move the same signal twice.
    let history_for_editor = query_ctx.history;
    let next_history_id_for_editor = query_ctx.next_history_id;
    let saved_queries_signal_for_editor = query_ctx.saved_queries;
    let next_saved_query_id_for_editor = query_ctx.next_saved_query_id;

    let editor_offset = format!(
        "transform: translate(-{}px, -{}px);",
        scroll_left(),
        scroll_top()
    );

    let schema_context = use_memo(use_reactive((&active_session_id,), move |(session_id,)| {
        build_schema_context(&explorer_sections(), session_id)
    }));

    use_effect(use_reactive(
        (&active_tab_id_value, &sql),
        move |(tab_id, next_sql)| {
            let first_sync = !*has_synced_editor_dom.peek();
            let tab_changed = *synced_editor_tab_id.peek() != tab_id;
            let draft_matches = {
                let current_sql = draft_sql.peek();
                current_sql.as_str() == next_sql.as_str()
            };
            if !first_sync && !tab_changed && draft_matches {
                return;
            }

            has_synced_editor_dom.set(true);
            synced_editor_tab_id.set(tab_id);
            draft_sql.set(next_sql.clone());
            editor_selection.set(EditorSelection::collapsed(next_sql.len()));
            is_typing.set(false);
            reset_completion_to_snapshot(
                completion_runtime,
                hash_completion_snapshot(&next_sql, next_sql.len()),
            );
            menu_items.set(Vec::new());
            menu_index.set(0);
            menu_force.set(false);
            menu_closed.set(false);
            menu_source_sql.set(String::new());
            let cursor = next_sql.len();
            spawn(async move {
                let _ = document::eval(&set_editor_value_script(
                    SQL_EDITOR_TEXTAREA_ID,
                    &next_sql,
                    cursor,
                    false,
                ))
                .join::<bool>()
                .await;
            });
        },
    ));

    use_effect(move || {
        if !is_typing() {
            return;
        }

        let revision = editor_revision();
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(HIGHLIGHT_IDLE_MS)).await;
            if editor_revision() == revision {
                is_typing.set(false);
            }
        });
    });

    use_effect(move || {
        let _ = crate::app_state::APP_FOCUS_EDITOR_REQUEST();
        let _ = document::eval(&format!(
            r#"
            (() => {{
                const editor = document.getElementById({id:?});
                if (editor) {{
                    editor.focus();
                }}
            }})()
            "#,
            id = SQL_EDITOR_TEXTAREA_ID
        ));
    });

    use_effect(move || {
        let revision = editor_revision();

        spawn(async move {
            tokio::time::sleep(Duration::from_millis(90)).await;
            if editor_revision() != revision {
                return;
            }

            let Ok((next_sql, start, end)) = document::eval(
                &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
            )
            .join::<(String, usize, usize)>()
            .await
            else {
                return;
            };
            let draft_changed = {
                let current_sql = draft_sql.peek();
                current_sql.as_str() != next_sql.as_str()
            };
            if draft_changed {
                draft_sql.set(next_sql.clone());
            }
            let next_selection = EditorSelection { start, end };
            let selection_changed = {
                let current_selection = editor_selection.peek();
                *current_selection != next_selection
            };
            if selection_changed {
                editor_selection.set(next_selection);
            }
            let already_synced = store
                .editor
                .read()
                .get(&active_tab_id_value)
                .is_some_and(|ed| ed.sql == next_sql);
            if already_synced {
                return;
            }

            sync_active_tab_sql_draft(store, active_tab_id_value, next_sql);
        });
    });

    use_effect(move || {
        let revision = editor_revision();
        let force = menu_force();
        let closed = menu_closed();
        let session_id = active_session_id;

        spawn(async move {
            let (sql_text, start, end) = if let Ok((sql, start, end)) = document::eval(
                &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
            )
            .join::<(String, usize, usize)>()
            .await
            {
                (sql, start, end)
            } else {
                let sql = draft_sql.peek().clone();
                let selection = editor_selection.peek().clamped(&sql);
                (sql, selection.start, selection.end)
            };
            if editor_revision() != revision {
                return;
            }
            if start != end {
                menu_items.set(Vec::new());
                menu_source_sql.set(String::new());
                return;
            }

            let query = parse_completion_query(&sql_text, start);
            let kind = APP_STATE()
                .session(session_id)
                .map(|session| session.kind)
                .unwrap_or(DatabaseKind::Sqlite);
            let nodes = explorer_sections
                .peek()
                .iter()
                .find(|section| section.session_id == session_id)
                .map(|section| section.nodes.clone())
                .unwrap_or_default();

            if !query.dotted.is_empty() {
                let table = query.dotted[query.dotted.len() - 1].clone();
                let schema =
                    (query.dotted.len() >= 2).then(|| query.dotted[query.dotted.len() - 2].clone());
                if table_missing_columns(&nodes, schema.as_deref(), &table) {
                    let key = (
                        session_id,
                        schema.clone().unwrap_or_default(),
                        table.clone(),
                    );
                    let already = column_fetches.peek().contains(&key);
                    if !already {
                        column_fetches.with_mut(|fetches| {
                            fetches.insert(key);
                        });
                        let dotted = query.dotted.clone();
                        spawn(async move {
                            let Ok(columns) = services::load_table_columns(
                                session_id,
                                schema.clone(),
                                table.clone(),
                            )
                            .await
                            else {
                                return;
                            };
                            let sql = draft_sql.peek().clone();
                            let cursor = editor_selection.peek().clamped(&sql).start;
                            let now = parse_completion_query(&sql, cursor);
                            if now.dotted != dotted {
                                return;
                            }
                            explorer_sections.with_mut(|sections| {
                                if let Some(section) = sections
                                    .iter_mut()
                                    .find(|section| section.session_id == session_id)
                                {
                                    merge_columns_into_tree(
                                        &mut section.nodes,
                                        schema.as_deref(),
                                        &table,
                                        &columns,
                                    );
                                }
                            });
                            editor_revision += 1;
                        });
                    }
                }
            }

            if closed && !force {
                menu_items.set(Vec::new());
                menu_source_sql.set(String::new());
                return;
            }

            let items = collect_menu_items(kind, &nodes, &query, force);
            if editor_revision() != revision {
                return;
            }
            let len = items.len();
            menu_items.set(items);
            menu_source_sql.set(sql_text);
            menu_index.set(if len == 0 {
                0
            } else {
                menu_index.peek().min(len - 1)
            });
            if len > 0 {
                spawn_caret_anchor_update(caret_anchor);
            }
        });
    });

    use_effect(move || {
        // Reading the signal is what subscribes the effect to it;
        // the value is not otherwise used in this block.
        let _ = editor_revision();
        let settings = APP_UI_SETTINGS();

        if !settings.sql_ghost_ready() {
            invalidate_completion(completion_runtime);
            return;
        }

        spawn(async move {
            tokio::time::sleep(Duration::from_millis(COMPLETION_DEBOUNCE_MS)).await;

            if completion_runtime.peek().discarded {
                return;
            }

            // Read SQL and caret from DOM (most accurate), fall back to signals.
            let (sql_text, start, end) = if let Ok((sql, start, end)) = document::eval(
                &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
            )
            .join::<(String, usize, usize)>()
            .await
            {
                (sql, start, end)
            } else {
                let sql = draft_sql.peek().clone();
                let selection = editor_selection.peek().clamped(&sql);
                (sql, selection.start, selection.end)
            };

            if start != end {
                eprintln!("[completion] bail: no cursor (selection range)");
                invalidate_completion(completion_runtime);
                return;
            }

            if sql_text.len() < 3 {
                eprintln!(
                    "[completion] bail: sql too short ({} chars)",
                    sql_text.len()
                );
                invalidate_completion(completion_runtime);
                return;
            }

            let selection = EditorSelection { start, end };
            let Some((cursor, prefix, suffix)) = completion_request_parts(&sql_text, selection)
            else {
                eprintln!("[completion] bail: no cursor (selection range)");
                invalidate_completion(completion_runtime);
                return;
            };

            // Re-check settings after debounce (they may have changed).
            let settings = APP_UI_SETTINGS();
            if !settings.sql_ghost_ready() {
                eprintln!("[completion] bail: settings changed, ghost not ready");
                invalidate_completion(completion_runtime);
                return;
            }

            let sql_hash = hash_completion_snapshot(&sql_text, cursor);
            let completion_snapshot = completion_runtime.peek().clone();
            if completion_snapshot.last_completed_snapshot == Some(sql_hash)
                && completion_snapshot.pending_snapshot.is_none()
            {
                eprintln!("[completion] bail: already completed for this snapshot");
                return;
            }

            let expected_id = completion_runtime.with_mut(|state| state.begin_request(sql_hash));
            let mut schema_ctx = schema_context();
            let surrounding = surrounding_sql_context(&sql_text, cursor);
            if !surrounding.is_empty() {
                use std::fmt::Write;
                let _ = write!(
                    schema_ctx,
                    "-- Surrounding SQL context (before cursor):\n-- {}",
                    surrounding.replace('\n', "\n-- ")
                );
            }
            let sql_for_result = sql_text.clone();

            log_completion(&format!(
                "streaming completion: prefix={} cursor={}",
                prefix.len(),
                cursor
            ));
            let mut token_rx = stream_sql_ghost(&settings, prefix, suffix, schema_ctx, &[]);

            let mut accumulated = String::new();
            let mut token_count = 0u32;
            while let Some(token) = token_rx.recv().await {
                token_count += 1;
                // If a newer request started, abandon this one.
                if completion_runtime.peek().request_id != expected_id {
                    log_completion("abandoned (newer request)");
                    return;
                }

                match token {
                    CompletionToken::Text(t) => {
                        accumulated.push_str(&t);
                        let trimmed =
                            trim_completion_for_cursor(&sql_for_result, cursor, &accumulated);
                        if !trimmed.is_empty() {
                            completion_runtime.with_mut(|state| {
                                state.set_active(
                                    expected_id,
                                    sql_hash,
                                    cursor,
                                    sql_for_result.clone(),
                                    accumulated.clone(),
                                );
                            });
                        }
                    }
                    CompletionToken::Error(e) => {
                        log_completion(&format!("error: {}", e));
                        toast_error(format!("Completion failed: {e}"));
                        completion_runtime.with_mut(|state| {
                            if state.finish_request(expected_id, sql_hash) {
                                state.active = None;
                            }
                        });
                        return;
                    }
                    CompletionToken::Done => {
                        log_completion(&format!(
                            "done: {} tokens, text={}",
                            token_count, accumulated
                        ));
                        let trimmed =
                            trim_completion_for_cursor(&sql_for_result, cursor, &accumulated);
                        if trimmed.is_empty() {
                            completion_runtime.with_mut(|state| {
                                if state.finish_request(expected_id, sql_hash) {
                                    state.active = None;
                                }
                            });
                        } else {
                            log_completion(&format!("got completion: {}", accumulated));
                            completion_runtime.with_mut(|state| {
                                state.set_active(
                                    expected_id,
                                    sql_hash,
                                    cursor,
                                    sql_for_result.clone(),
                                    accumulated,
                                );
                            });
                        }
                        return;
                    }
                }
            }
            log_completion(&format!("channel closed: {} tokens", token_count));
        });
    });

    let typing_now = is_typing();
    let active_completion = completion_runtime().active;
    let render_completion = active_completion.as_ref().filter(|completion| {
        let cursor = completion.cursor.min(completion.source_sql.len());
        !completion.text.is_empty()
            && !trim_completion_for_cursor(&completion.source_sql, cursor, &completion.text)
                .is_empty()
    });
    let current_sql = render_completion
        .map(|completion| completion.source_sql.clone())
        .unwrap_or_else(|| {
            if typing_now {
                draft_sql.peek().clone()
            } else {
                draft_sql()
            }
        });
    let editor_class = if typing_now {
        "sql-editor sql-editor--typing"
    } else {
        "sql-editor"
    };
    let editor = APP_EDITOR_BEHAVIOR();
    let wrap = if editor.word_wrap { "pre-wrap" } else { "pre" };
    let editor_style = format!(
        "font-size: {}px; tab-size: {}; white-space: {}; font-family: var(--font-mono, monospace);",
        editor.font_size.clamp(10, 22),
        editor.tab_size.clamp(1, 8),
        wrap,
    );
    let highlight_style = format!("{editor_style}{editor_offset}");
    let gutter_offset = format!("transform: translateY(-{}px);", scroll_top());
    let inline_cursor =
        render_completion.map_or(0, |completion| completion.cursor.min(current_sql.len()));
    let inline_suffix = render_completion.map(|completion| {
        trim_completion_for_cursor(&current_sql, inline_cursor, &completion.text)
    });
    let completion_active = inline_suffix
        .as_ref()
        .is_some_and(|completion| !completion.is_empty());
    let inline_cursor_position = completion_active.then_some(inline_cursor);
    let menu_now = menu_items();
    let menu_height = menu_height_for_items(menu_now.len());
    let caret = caret_anchor();
    let (menu_left, menu_top, _) = autocomplete_offset(
        caret.x,
        caret.y,
        caret.line_height,
        menu_height,
        caret.editor_height,
        caret.editor_width,
        MENU_WIDTH,
    );
    let menu_active_index = menu_index();

    rsx! {
        div {
            class: editor_class.to_string(),
            style: editor_style.clone(),

            if editor.show_line_numbers {
                div {
                    class: "sql-editor__gutter",
                    div {
                        class: "sql-editor__gutter-inner",
                        style: gutter_offset,
                        for n in line_number_labels(&current_sql) {
                            span { "{n}" }
                        }
                    }
                }
            }

            div {
                class: "sql-editor__viewport",
                pre {
                    class: "sql-editor__highlight",
                    style: highlight_style,
                    aria_hidden: "true",
                    if !typing_now || completion_active {
                        SqlHighlightContent {
                            sql: current_sql.clone(),
                            inline_cursor_position,
                            inline_suffix,
                        }
                    }
                }

                textarea {
                    id: SQL_EDITOR_TEXTAREA_ID,
                    class: "sql-editor__input",
                    style: editor_style,
                    initial_value: current_sql.to_string(),
                rows: "16",
                cols: "80",
                spellcheck: "false",
                // Right-click opens the SQL editor's context menu
                // with copy/paste/format/run entries. We rely on
                // `prevent_default` to suppress the browser's
                // built-in menu because the app's menu has more
                // domain-specific actions (format, run, explain).
                oncontextmenu: move |event| {
                    event.prevent_default();
                    let coords = event.client_coordinates();
                    open_sql_editor_context_menu(
                        coords.x,
                        coords.y,
                        store,
                        active_tab_id_value,
                        saved_queries_signal_for_editor,
                        next_saved_query_id_for_editor,
                        history_for_editor,
                        next_history_id_for_editor,
                        acp_ctx.clone(),
                    );
                },

                oninput: move |event| {
                    let next_sql = event.value();
                    let draft_changed = {
                        let current_sql = draft_sql.peek();
                        current_sql.as_str() != next_sql.as_str()
                    };
                    if draft_changed {
                        // Keep the render snapshot aligned with the live textarea so the
                        // highlight layer never wakes up with stale SQL after the typing debounce.
                        draft_sql.set(next_sql.clone());
                        sync_active_tab_sql_draft(store, active_tab_id_value, next_sql);
                    }
                    let already_typing = {
                        let typing = is_typing.peek();
                        *typing
                    };
                    if !already_typing {
                        is_typing.set(true);
                    }
                    completion_runtime.with_mut(CompletionRuntime::clear_on_typing);
                    menu_closed.set(false);
                    editor_revision += 1;
                },

                onkeydown: move |event| {
                    // ─── Shortcuts (Ctrl/Cmd + key) ────────────────
                    // We intercept these BEFORE the completion
                    // handler so the user can press them anywhere
                    // in the editor, regardless of whether the
                    // completion popup is open.
                    let mods = event.modifiers();
                    let ctrl_or_meta = mods.contains(Modifiers::CONTROL)
                        || mods.contains(Modifiers::META);
                    if ctrl_or_meta {
                        match event.key() {
                            Key::Enter => {
                                // Ctrl+Enter — run the query.
                                event.prevent_default();
                                run_active_tab(
                                    store,
                                    active_tab_id_value,
                                    (history_for_editor, next_history_id_for_editor),
                                );
                                return;
                            }
                            Key::Character(ref c) if c == "/" => {
                                // Ctrl+/ — toggle line comments.
                                event.prevent_default();
                                let sel = event_selection_range(&event);
                                toggle_line_comments_in_active_tab(
                                    store,
                                    active_tab_id_value,
                                    sel.start..sel.end,
                                );
                                return;
                            }
                            Key::Character(ref c) if (c == "l" || c == "L")
                                && !mods.contains(Modifiers::SHIFT) => {
                                    // Ctrl+L — clear editor.
                                    event.prevent_default();
                                    clear_active_tab_sql(store, active_tab_id_value);
                                    return;
                                }
                            Key::Character(ref c) if (c == "s" || c == "S")
                                && !mods.contains(Modifiers::SHIFT) => {
                                    // Ctrl+S — save as saved query.
                                    event.prevent_default();
                                    let status = save_active_tab_as_saved_query(
                                        store,
                                        active_tab_id_value,
                                        saved_queries_signal_for_editor,
                                        next_saved_query_id_for_editor,
                                    );
                                    if let Some(message) = status.strip_prefix("Saved ").and_then(|s| s.strip_suffix(".")) {
                                        use crate::app_state::{show_toast, ToastKind};
                                        show_toast(message.to_string(), ToastKind::Success);
                                    }
                                    return;
                                }
                            _ => {}
                        }
                        if mods.contains(Modifiers::SHIFT) {
                            match event.key() {
                                Key::Character(ref c) if c == "F" || c == "f" => {
                                    // Ctrl+Shift+F — format SQL.
                                    event.prevent_default();
                                    format_active_tab(store, active_tab_id_value, APP_SQL_FORMAT_SETTINGS());
                                    return;
                                }
                                Key::Character(ref c) if c == "E" || c == "e" => {
                                    // Ctrl+Shift+E — explain query.
                                    event.prevent_default();
                                    run_active_tab_explain(store, active_tab_id_value);
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }

                    let menu_open = !menu_items.peek().is_empty();
                    let ghost_visible = completion_runtime
                        .peek()
                        .active
                        .as_ref()
                        .is_some_and(|completion| !completion.text.is_empty());
                    let action =
                        editor_completion_action(map_completion_key(&event), menu_open, ghost_visible);

                    match action {
                        EditorKeyAction::Pass => {}
                        EditorKeyAction::CloseMenu => {
                            event.prevent_default();
                            menu_items.set(Vec::new());
                            menu_index.set(0);
                            menu_force.set(false);
                            menu_closed.set(true);
                            menu_source_sql.set(String::new());
                        }
                        EditorKeyAction::DismissGhost => {
                            event.prevent_default();
                            completion_runtime.with_mut(CompletionRuntime::dismiss_ghost);
                        }
                        EditorKeyAction::CycleGhostNext => {
                            event.prevent_default();
                            cycle_ghost_next(
                                completion_runtime,
                                draft_sql,
                                editor_selection,
                                schema_context(),
                            );
                        }
                        EditorKeyAction::CycleGhostPrev => {
                            event.prevent_default();
                            completion_runtime.with_mut(|state| {
                                state.variants.prev();
                                if let Some(text) = state.variants.current().map(str::to_string)
                                    && let Some(active) = &mut state.active
                                {
                                    active.text = text;
                                }
                            });
                        }
                        EditorKeyAction::MenuMove(delta) => {
                            event.prevent_default();
                            let len = menu_items.peek().len();
                            if len > 0 {
                                menu_index.with_mut(|index| {
                                    *index = (*index as i32 + delta).rem_euclid(len as i32) as usize;
                                });
                            }
                        }
                        EditorKeyAction::AcceptMenu => {
                            event.prevent_default();
                            let item = {
                                let items = menu_items.peek();
                                items.get(*menu_index.peek()).cloned()
                            };
                            if let Some(item) = item {
                                accept_completion_menu_item(
                                    item,
                                    store,
                                    active_tab_id_value,
                                    draft_sql,
                                    editor_selection,
                                    is_typing,
                                    editor_revision,
                                    completion_runtime,
                                    menu_items,
                                    menu_index,
                                    menu_force,
                                    menu_closed,
                                    menu_source_sql,
                                );
                            }
                        }
                        EditorKeyAction::AcceptGhost => {
                            event.prevent_default();
                            let completion_text_raw = {
                                let state = completion_runtime.peek();
                                state
                                    .variants
                                    .current()
                                    .map(str::to_string)
                                    .or_else(|| state.active.as_ref().map(|active| active.text.clone()))
                            };
                            if let Some(completion_text_raw) = completion_text_raw {
                                apply_inline_completion(
                                    completion_runtime,
                                    store,
                                    active_tab_id_value,
                                    draft_sql,
                                    editor_selection,
                                    is_typing,
                                    editor_revision,
                                    completion_text_raw,
                                    "tab",
                                );
                            }
                        }
                        EditorKeyAction::Indent { shift } => {
                            let direction = if shift {
                                IndentDirection::Out
                            } else {
                                IndentDirection::In
                            };
                            let sel = event_selection_range(&event);
                            indent_lines_in_active_tab(
                                store,
                                active_tab_id_value,
                                sel.start..sel.end,
                                direction,
                            );
                        }
                        EditorKeyAction::ForceMenu => {
                            event.prevent_default();
                            menu_closed.set(false);
                            menu_force.set(true);
                            editor_revision += 1;
                        }
                    }
                },

                onkeyup: move |event| {
                    match event.key() {
                        Key::ArrowLeft
                        | Key::ArrowRight
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::Home
                        | Key::End
                        | Key::PageUp
                        | Key::PageDown => {
                            editor_revision += 1;
                            sync_editor_selection_debounced(editor_selection, SQL_EDITOR_TEXTAREA_ID);
                        }
                        _ => {}
                    }
                },

                onmouseup: move |_| {
                    editor_revision += 1;
                    sync_editor_selection_debounced(editor_selection, SQL_EDITOR_TEXTAREA_ID);
                },

                onclick: move |_| {
                    editor_revision += 1;
                    sync_editor_selection_debounced(editor_selection, SQL_EDITOR_TEXTAREA_ID);
                },

                onfocus: move |_| {
                    sync_editor_selection(editor_selection, SQL_EDITOR_TEXTAREA_ID);
                },

                onscroll: move |event| {
                    scroll_top.set(event.data().scroll_top());
                    scroll_left.set(event.data().scroll_left());
                    if should_refresh_menu_caret(menu_items.peek().len()) {
                        spawn_caret_anchor_update(caret_anchor);
                    }
                },
            }
            }

            if !menu_now.is_empty() {
                SqlCompletionMenu {
                    items: menu_now.clone(),
                    active_index: menu_active_index,
                    left: menu_left,
                    top: menu_top,
                    max_height: menu_height,
                    on_accept: move |index| {
                        let item = menu_items.peek().get(index).cloned();
                        if let Some(item) = item {
                            accept_completion_menu_item(
                                item,
                                store,
                                active_tab_id_value,
                                draft_sql,
                                editor_selection,
                                is_typing,
                                editor_revision,
                                completion_runtime,
                                menu_items,
                                menu_index,
                                menu_force,
                                menu_closed,
                                menu_source_sql,
                            );
                        }
                    },
                }
            }
        }
    }
}
