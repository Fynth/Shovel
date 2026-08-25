#[path = "sql_editor/highlight.rs"]
mod highlight;
#[path = "sql_editor/selection.rs"]
mod selection;

use crate::{
    app_state::{
        APP_AI_AUTO_APPLY_COMPLETIONS,
        APP_AI_FEATURES_ENABLED,
        APP_SQL_FORMAT_SETTINGS,
        APP_UI_SETTINGS,
        context_menu::{ContextMenuItem, open_context_menu},
        toast_error,
    },
    completion::{CompletionService, CompletionToken},
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
use models::{ExplorerNodeKind, QueryHistoryItem};
use std::time::Duration;

use self::{
    highlight::SqlHighlightContent,
    selection::{
        EditorSelection,
        current_token_range,
        editor_value_and_selection_query_script,
        set_editor_value_script,
        sync_editor_selection,
        sync_editor_selection_debounced,
    },
};

const SQL_EDITOR_TEXTAREA_ID: &str = "workspace-sql-editor";
const COMPLETION_DEBOUNCE_MS: u64 = 180;
const HIGHLIGHT_IDLE_MS: u64 = 90;
/// Idle pause before a finished inline completion is auto-inserted.
/// Typing during this window cancels the auto-apply.
const AUTO_APPLY_IDLE_MS: u64 = 400;

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
}

impl CompletionRuntime {
    fn invalidate(&mut self) {
        self.request_id = self.request_id.wrapping_add(1);
        self.pending_snapshot = None;
        self.last_completed_snapshot = None;
        self.active = None;
    }

    fn reset_to_snapshot(&mut self, snapshot: usize) {
        self.invalidate();
        self.last_completed_snapshot = Some(snapshot);
    }

    fn begin_request(&mut self, snapshot: usize) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.pending_snapshot = Some(snapshot);
        self.active = None;
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
        if self.finish_request(request_id, snapshot) {
            self.active = Some(InlineCompletion {
                cursor,
                source_sql,
                text,
            });
        }
    }
}

fn invalidate_completion(mut completion: Signal<CompletionRuntime>) {
    completion.with_mut(CompletionRuntime::invalidate);
}

fn invalidate_active_completion(mut completion: Signal<CompletionRuntime>) {
    completion.with_mut(|state| {
        if state.active.is_some() || state.pending_snapshot.is_some() {
            state.invalidate();
        }
    });
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

fn is_completion_accept_key(event: &KeyboardEvent) -> bool {
    event.key() == Key::Tab || event.code() == Code::Tab
}

/// Insert the active inline completion into the editor. Used by both
/// the Tab accept handler and the auto-apply idle timer so the two
/// insertion paths stay byte-identical (same trim rules, same clause
/// space handling, same DOM/state sync).
///
/// `source` is a short tag used only for logging and tracing
/// (`"tab"` vs `"auto"`).
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
        // Read current SQL from DOM (most accurate), fall back to signal.
        let actual_sql = if let Ok((sql, _, _)) = document::eval(
            &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
        )
        .join::<(String, usize, usize)>()
        .await
        {
            sql
        } else {
            draft_sql.peek().clone()
        };
        let cursor = actual_sql.len();
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

/// Schedule an automatic insert for the active inline completion. After
/// `AUTO_APPLY_IDLE_MS` of idle time (no typing, no newer request, no
/// dismissal) the completion is applied via the same path Tab uses.
///
/// The timer is keyed on the editor's `editor_revision` plus the
/// completion snapshot: any typing (which bumps `editor_revision`)
/// cancels the pending insert, so the editor never "fights" a user
/// who's still editing.
///
/// The setting `ai_auto_apply_completions` gates the timer; when it
/// is off, the timer is a no-op and completions stay as ghost text
/// until the user accepts manually.
#[allow(clippy::too_many_arguments)]
fn schedule_auto_apply(
    completion_runtime: Signal<CompletionRuntime>,
    store: TabStore,
    active_tab_id_value: u64,
    draft_sql: Signal<String>,
    editor_selection: Signal<EditorSelection>,
    is_typing: Signal<bool>,
    editor_revision: Signal<u64>,
    snapshot: usize,
) {
    if !APP_AI_AUTO_APPLY_COMPLETIONS() {
        log_completion("auto-apply: disabled by setting");
        return;
    }

    // Snapshot the completion text now so a later request (e.g. the user
    // hitting Tab and triggering another fetch) can't race us into
    // inserting the wrong completion.
    let completion_text = {
        let state = completion_runtime.peek();
        match state.active.as_ref() {
            Some(active) if state.last_completed_snapshot == Some(snapshot) => active.text.clone(),
            _ => return,
        }
    };

    // Re-check after taking the snapshot: if the user typed between the
    // Done token and us reaching this point, editor_revision already
    // moved on and there is nothing to auto-apply.
    let revision_at_schedule = *editor_revision.peek();
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(AUTO_APPLY_IDLE_MS)).await;

        // Typing (or any other revision bump) during the idle window
        // cancels the auto-apply — the user is still editing.
        if *editor_revision.peek() != revision_at_schedule {
            log_completion("auto-apply: cancelled by typing");
            return;
        }

        // Confirm the same completion is still active and the
        // completion runtime hasn't been invalidated (e.g. by a newer
        // request or an explicit Esc dismiss).
        let should_apply = {
            let state = completion_runtime.peek();
            state.last_completed_snapshot == Some(snapshot)
                && state.active.is_some()
                && state.pending_snapshot.is_none()
        };
        if !should_apply {
            log_completion("auto-apply: cancelled (completion no longer active)");
            return;
        }

        apply_inline_completion(
            completion_runtime,
            store,
            active_tab_id_value,
            draft_sql,
            editor_selection,
            is_typing,
            editor_revision,
            completion_text,
            "auto",
        );
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
    use super::{completion_request_parts, selection::EditorSelection, trim_completion_for_cursor};

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
    fn trim_completion_removes_repeated_token_and_suffix_overlap() {
        let sql = "sel from users";
        let cursor = "sel".len();

        assert_eq!(
            trim_completion_for_cursor(sql, cursor, "select from users"),
            "ect"
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

fn trim_completion_for_cursor(sql: &str, cursor: usize, completion: &str) -> String {
    let mut completion = completion
        .trim_matches(|ch| matches!(ch, '\r' | '\n'))
        .to_string();
    if completion.is_empty() {
        return completion;
    }

    let token_range = current_token_range(sql, EditorSelection::collapsed(cursor));
    let typed_token = &sql[token_range.start..cursor];
    if !typed_token.is_empty() && completion.starts_with(typed_token) {
        completion = completion[typed_token.len()..].to_string();
    }

    let suffix = &sql[cursor..];
    let prefix_overlap = common_prefix_byte_len(suffix, &completion);
    if prefix_overlap > 0 {
        completion = completion[prefix_overlap..].to_string();
    }

    let suffix_overlap = suffix_prefix_overlap_byte_len(suffix, &completion);
    if suffix_overlap > 0 {
        completion.truncate(completion.len() - suffix_overlap);
    }

    completion
}

fn common_prefix_byte_len(left: &str, right: &str) -> usize {
    let mut byte_len = 0;
    for (left_ch, right_ch) in left.chars().zip(right.chars()) {
        if left_ch != right_ch {
            break;
        }
        byte_len += right_ch.len_utf8();
    }
    byte_len
}

fn suffix_prefix_overlap_byte_len(suffix: &str, completion: &str) -> usize {
    let mut best_overlap = 0;
    let mut suffix_prefix_len = 0;
    for ch in suffix.chars() {
        suffix_prefix_len += ch.len_utf8();
        if completion.ends_with(&suffix[..suffix_prefix_len]) {
            best_overlap = suffix_prefix_len;
        }
    }
    best_overlap
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
        // Reading the signal is what subscribes the effect to it;
        // the value is not otherwise used in this block.
        let _ = editor_revision();
        let settings = APP_UI_SETTINGS();
        let completion_service = CompletionService::new(&settings);

        if completion_service.is_empty() {
            invalidate_completion(completion_runtime);
            return;
        }

        spawn(async move {
            tokio::time::sleep(Duration::from_millis(COMPLETION_DEBOUNCE_MS)).await;

            // Read SQL from DOM (most accurate), fall back to signals.
            let sql_text = if let Ok((sql, _, _)) = document::eval(
                &editor_value_and_selection_query_script(SQL_EDITOR_TEXTAREA_ID),
            )
            .join::<(String, usize, usize)>()
            .await
            {
                sql
            } else {
                draft_sql.peek().clone()
            };

            if sql_text.len() < 3 {
                eprintln!(
                    "[completion] bail: sql too short ({} chars)",
                    sql_text.len()
                );
                invalidate_completion(completion_runtime);
                return;
            }

            // Complete at the end of the SQL — most reliable position.
            let cursor = sql_text.len();
            let selection = EditorSelection::collapsed(cursor);

            let Some((cursor, prefix, suffix)) = completion_request_parts(&sql_text, selection)
            else {
                eprintln!("[completion] bail: no cursor (selection range)");
                invalidate_completion(completion_runtime);
                return;
            };

            // Re-check settings after debounce (they may have changed).
            if CompletionService::new(&APP_UI_SETTINGS()).is_empty() {
                eprintln!("[completion] bail: settings changed, no providers");
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

            // Stream completion tokens from the AI provider.
            // Tokens arrive incrementally and are shown as ghost text immediately.
            log_completion(&format!(
                "streaming completion: prefix={} cursor={}",
                prefix.len(),
                cursor
            ));
            let mut token_rx = completion_service.stream_completion(prefix, suffix, schema_ctx);

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
                        // Show partial completion immediately (Zed-style).
                        let trimmed =
                            trim_completion_for_cursor(&sql_for_result, cursor, &accumulated);
                        if !trimmed.is_empty() {
                            completion_runtime.with_mut(|state| {
                                state.active = Some(InlineCompletion {
                                    cursor,
                                    source_sql: sql_for_result.clone(),
                                    text: accumulated.clone(),
                                });
                            });
                        }
                    }
                    CompletionToken::Error(e) => {
                        log_completion(&format!("error: {}", e));
                        toast_error(format!("Completion failed: {e}"));
                        completion_runtime.with_mut(|state| {
                            state.finish_request(expected_id, sql_hash);
                        });
                        return;
                    }
                    CompletionToken::Done => {
                        log_completion(&format!(
                            "done: {} tokens, text={}",
                            token_count, accumulated
                        ));
                        // Finalize: only keep the completion if it's non-empty after trimming.
                        let trimmed =
                            trim_completion_for_cursor(&sql_for_result, cursor, &accumulated);
                        if trimmed.is_empty() {
                            completion_runtime.with_mut(|state| {
                                state.finish_request(expected_id, sql_hash);
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
                            schedule_auto_apply(
                                completion_runtime,
                                store,
                                active_tab_id_value,
                                draft_sql,
                                editor_selection,
                                is_typing,
                                editor_revision,
                                sql_hash,
                            );
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
    let inline_cursor =
        render_completion.map_or(0, |completion| completion.cursor.min(current_sql.len()));
    let inline_suffix = render_completion.map(|completion| {
        trim_completion_for_cursor(&current_sql, inline_cursor, &completion.text)
    });
    let completion_active = inline_suffix
        .as_ref()
        .is_some_and(|completion| !completion.is_empty());
    let inline_cursor_position = completion_active.then_some(inline_cursor);

    rsx! {
        div {
            class: "{editor_class}",

            div {
                class: "sql-editor__viewport",
                pre {
                    class: "sql-editor__highlight",
                    style: "{editor_offset}",
                    aria_hidden: "true",
                    if !typing_now || completion_active {
                        SqlHighlightContent {
                            sql: current_sql.clone(),
                            inline_cursor_position,
                            inline_suffix,
                        }
                    }
                }
            }

            textarea {
                id: SQL_EDITOR_TEXTAREA_ID,
                class: "sql-editor__input",
                initial_value: "{current_sql}",
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
                    invalidate_active_completion(completion_runtime);
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

                    // Tab with no modifier — indent / outdent selected
                    // lines. We also fall through to the
                    // completion-accept logic below.
                    if event.key() == Key::Tab && !ctrl_or_meta {
                        let direction = if mods.contains(Modifiers::SHIFT) {
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
                        // Don't return: let the default Tab behaviour
                        // still happen so the cursor advances
                        // naturally after indenting.
                    }

                    let active_completion = {
                        let completion_state = completion_runtime.peek();
                        completion_state.active.clone()
                    };

                    if is_completion_accept_key(&event)
                        && let Some(completion_state) = active_completion.clone()
                        && !completion_state.text.is_empty()
                    {
                        event.prevent_default();
                        let completion_text_raw = completion_state.text.clone();
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
                },
            }
        }
    }
}
