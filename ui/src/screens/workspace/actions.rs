use crate::app_state::{
    APP_EDITOR_BEHAVIOR,
    APP_READ_ONLY_MODE,
    APP_SQL_FORMAT_SETTINGS,
    APP_STATE,
    APP_UI_SETTINGS,
    LastQuerySummary,
    activate_session,
    set_last_query,
    set_show_sql_editor,
};
use dioxus::prelude::*;
use models::{
    BatchOutcome,
    BatchResult,
    BatchRunState,
    BatchTransactionState,
    DatabaseKind,
    PendingTableChanges,
    QueryFilter,
    QueryFilterMode,
    QueryHistoryItem,
    QueryOutput,
    QuerySort,
    QueryTabState,
    TablePreviewSource,
    WorkspaceTabKind,
};
use std::time::Instant;

use super::{
    helpers::{can_explain, session_capabilities},
    tab_store::{
        TabEditorState,
        TabMeta,
        TabPendingState,
        TabResultState,
        TabStore,
        tab_editor,
        tab_meta,
        tab_pending,
        tab_result,
    },
};

fn redact_sql(sql: &str) -> String {
    let lower = sql.to_lowercase();
    if lower.contains("password") || lower.contains("secret") || lower.contains("token") {
        let mut result = sql.to_string();
        for sensitive in ["password", "secret", "token"] {
            if lower.contains(sensitive) {
                result = result
                    .lines()
                    .map(|line| {
                        let line_lower = line.to_lowercase();
                        if line_lower.contains(sensitive) {
                            if let Some(eq_pos) = line.find('=') {
                                let (before, _) = line.split_at(eq_pos + 1);
                                format!("{} [REDACTED]", before.trim_end())
                            } else {
                                line.to_string()
                            }
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
        result
    } else {
        sql.to_string()
    }
}

fn kind_type_label(kind: DatabaseKind) -> String {
    match kind {
        DatabaseKind::Sqlite => "sqlite".to_string(),
        DatabaseKind::Postgres => "postgres".to_string(),
        DatabaseKind::MySql => "mysql".to_string(),
        DatabaseKind::ClickHouse => "clickhouse".to_string(),
    }
}

fn connection_type_for_session(session_id: u64) -> String {
    APP_STATE
        .read()
        .session(session_id)
        .map(|session| kind_type_label(session.kind))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Возвращает семейство БД для планирования пакетного выполнения.
fn connection_family_for_session(session_id: u64) -> services::DatabaseFamily {
    match APP_STATE
        .read()
        .session(session_id)
        .map(|session| session.kind)
    {
        Some(DatabaseKind::Postgres) => services::DatabaseFamily::Postgres,
        Some(DatabaseKind::MySql) => services::DatabaseFamily::MySql,
        Some(DatabaseKind::ClickHouse) => services::DatabaseFamily::ClickHouse,
        Some(DatabaseKind::Sqlite) | None => services::DatabaseFamily::Sqlite,
    }
}

/// Краткое превью оператора для вкладки пакетного результата: первая
/// непустая строка, обрезанная до 80 символов.
fn preview_statement(sql: &str) -> String {
    let first = sql
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut out = first.to_string();
    if out.chars().count() > 80 {
        out = out.chars().take(77).collect::<String>() + "…";
    }
    out
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

type QueryHistorySignals = (Signal<Vec<QueryHistoryItem>>, Signal<u64>, String, String);

pub fn read_only_mode_enabled() -> bool {
    APP_READ_ONLY_MODE()
}

pub fn read_only_mode_blocks_sql(sql: &str) -> bool {
    read_only_mode_enabled() && !services::is_read_only_sql(sql)
}

pub fn read_only_mode_block_status(action: &str) -> String {
    format!("Read-only mode blocked {action}. Disable read-only mode in Settings to allow writes.")
}

pub fn new_query_tab(
    id: u64,
    session_id: u64,
    title: String,
    sql: String,
) -> (TabMeta, TabEditorState, TabResultState, TabPendingState) {
    let page_size = APP_UI_SETTINGS().default_page_size;
    (
        tab_meta(id, session_id, title, WorkspaceTabKind::Query, false),
        tab_editor(sql),
        tab_result(page_size),
        tab_pending(),
    )
}

pub fn ensure_tab_for_session(mut store: TabStore, session_id: u64) -> u64 {
    activate_session(session_id);
    let mut active_tab_id = store.active_tab_id;
    let mut next_tab_id = store.next_tab_id;

    if let Some(existing_id) = store
        .meta
        .read()
        .iter()
        .find(|(_, t)| t.session_id == session_id && t.tab_kind == WorkspaceTabKind::Query)
        .map(|(id, _)| *id)
    {
        active_tab_id.set(existing_id);
        return existing_id;
    }

    let tab_id = next_tab_id();
    next_tab_id += 1;
    let (meta, editor, result, pending) = new_query_tab(
        tab_id,
        session_id,
        format!("Query {tab_id}"),
        "select 1 as id;".to_string(),
    );
    store.meta.with_mut(|m| {
        m.insert(tab_id, meta);
    });
    store.editor.with_mut(|m| {
        m.insert(tab_id, editor);
    });
    store.result.with_mut(|m| {
        m.insert(tab_id, result);
    });
    store.pending.with_mut(|m| {
        m.insert(tab_id, pending);
    });
    active_tab_id.set(tab_id);
    tab_id
}

/// Pick the tab that should host a table preview for `session_id`.
///
/// Prefers an existing preview of the same table so double-clicking an
/// explorer node does not keep spawning empty Query tabs. Falls back to a
/// scratch Query tab for the session, then to creating a new tab.
pub fn ensure_tab_for_table_preview(
    mut store: TabStore,
    session_id: u64,
    source: &TablePreviewSource,
) -> u64 {
    activate_session(session_id);
    let mut active_tab_id = store.active_tab_id;
    let mut next_tab_id = store.next_tab_id;

    let metas = store
        .meta
        .read()
        .iter()
        .map(|(id, meta)| (*id, meta.session_id, meta.tab_kind))
        .collect::<Vec<_>>();
    let candidates = metas
        .into_iter()
        .map(|(id, tab_session_id, tab_kind)| {
            let preview_qualified_name = store
                .result
                .read()
                .get(&id)
                .and_then(|res| res.preview_source.as_ref())
                .map(|preview| preview.qualified_name.clone());
            PreviewTabCandidate {
                id,
                session_id: tab_session_id,
                tab_kind,
                preview_qualified_name,
            }
        })
        .collect::<Vec<_>>();

    if let Some(existing_id) =
        resolve_table_preview_tab_id(&candidates, session_id, &source.qualified_name)
    {
        active_tab_id.set(existing_id);
        return existing_id;
    }

    let tab_id = next_tab_id();
    next_tab_id += 1;
    let (meta, editor, result, pending) =
        new_query_tab(tab_id, session_id, source.table_name.clone(), String::new());
    store.meta.with_mut(|m| {
        m.insert(tab_id, meta);
    });
    store.editor.with_mut(|m| {
        m.insert(tab_id, editor);
    });
    store.result.with_mut(|m| {
        m.insert(tab_id, result);
    });
    store.pending.with_mut(|m| {
        m.insert(tab_id, pending);
    });
    active_tab_id.set(tab_id);
    tab_id
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewTabCandidate {
    pub id: u64,
    pub session_id: u64,
    pub tab_kind: WorkspaceTabKind,
    pub preview_qualified_name: Option<String>,
}

pub(crate) fn resolve_table_preview_tab_id(
    tabs: &[PreviewTabCandidate],
    session_id: u64,
    qualified_name: &str,
) -> Option<u64> {
    tabs.iter()
        .find(|tab| {
            tab.session_id == session_id
                && tab.preview_qualified_name.as_deref() == Some(qualified_name)
        })
        .or_else(|| {
            tabs.iter()
                .find(|tab| tab.session_id == session_id && tab.tab_kind == WorkspaceTabKind::Query)
        })
        .map(|tab| tab.id)
}

pub fn update_active_tab_sql(mut store: TabStore, active_tab_id: u64, sql: String, status: String) {
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&active_tab_id) {
            if ed.sql != sql {
                store.result.with_mut(|r| {
                    if let Some(res) = r.get_mut(&active_tab_id) {
                        res.show_execution_plan = false;
                    }
                });
            }
            ed.sql = sql;
        }
    });
    store.result.with_mut(|r| {
        if let Some(res) = r.get_mut(&active_tab_id) {
            res.status = status;
            res.result = None;
            res.current_offset = 0;
            res.last_run_sql = None;
            res.preview_source = None;
            res.filter = None;
            res.sort = None;
            res.is_loading_more = false;
        }
    });
    store.meta.with_mut(|m| {
        if let Some(meta) = m.get_mut(&active_tab_id) {
            meta.tab_kind = WorkspaceTabKind::Query;
        }
    });
    store.pending.with_mut(|m| {
        if let Some(p) = m.get_mut(&active_tab_id) {
            p.pending_table_changes = PendingTableChanges::default();
        }
    });
}

#[cfg(test)]
fn sync_tab_sql_draft(tab: &mut QueryTabState, sql: &str) {
    if tab.sql == sql {
        return;
    }

    tab.sql = sql.to_string();
    tab.show_execution_plan = false;
}

pub fn sync_active_tab_sql_draft(mut store: TabStore, active_tab_id: u64, sql: String) {
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&active_tab_id) {
            if ed.sql == sql {
                return;
            }
            ed.sql = sql;
            store.result.with_mut(|r| {
                if let Some(res) = r.get_mut(&active_tab_id) {
                    res.show_execution_plan = false;
                }
            });
        }
    });
}

pub fn set_active_tab_sql(store: TabStore, active_tab_id: u64, sql: String, status: String) {
    update_active_tab_sql(store, active_tab_id, sql, status);
}

pub fn append_to_tab_sql(mut store: TabStore, tab_id: u64, sql_fragment: String, status: String) {
    let current_sql = store.editor.read().get(&tab_id).map(|ed| ed.sql.clone());
    let Some(current_sql) = current_sql else {
        return;
    };

    let new_sql = if current_sql.trim().is_empty() {
        sql_fragment
    } else if sql_fragment.trim().is_empty() {
        return;
    } else if current_sql.ends_with('\n') {
        format!("{current_sql}{sql_fragment}")
    } else {
        format!("{current_sql}\n\n{sql_fragment}")
    };

    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&tab_id) {
            ed.sql = new_sql;
        }
    });
    store.result.with_mut(|r| {
        if let Some(res) = r.get_mut(&tab_id) {
            res.status = status;
            res.result = None;
            res.current_offset = 0;
            res.last_run_sql = None;
            res.preview_source = None;
            res.filter = None;
            res.sort = None;
            res.is_loading_more = false;
        }
    });
    store.meta.with_mut(|m| {
        if let Some(meta) = m.get_mut(&tab_id) {
            meta.tab_kind = WorkspaceTabKind::Query;
        }
    });
    store.pending.with_mut(|m| {
        if let Some(p) = m.get_mut(&tab_id) {
            p.pending_table_changes = PendingTableChanges::default();
        }
    });
}

pub fn set_active_tab_status(mut store: TabStore, active_tab_id: u64, status: String) {
    store.result.with_mut(|r| {
        if let Some(res) = r.get_mut(&active_tab_id) {
            res.status = status;
        }
    });
}

#[cfg(test)]
fn toggle_cached_execution_plan(tab: &mut QueryTabState, sql: &str) -> bool {
    if tab.show_execution_plan && tab.execution_plan.is_some() {
        tab.show_execution_plan = false;
        return true;
    }

    let normalized_sql = sql.trim();
    let can_reopen_cached_plan = tab.execution_plan.as_ref().is_some_and(|plan| {
        !normalized_sql.is_empty() && plan.explained_sql.trim() == normalized_sql
    });
    if can_reopen_cached_plan {
        tab.show_execution_plan = true;
        return true;
    }

    false
}

pub fn toggle_execution_plan_for_tab(mut store: TabStore, active_tab_id: u64, sql: &str) -> bool {
    let mut handled = false;
    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&active_tab_id) {
            if res.show_execution_plan && res.execution_plan.is_some() {
                res.show_execution_plan = false;
                handled = true;
                return;
            }
            let normalized_sql = sql.trim();
            let can_reopen_cached_plan = res.execution_plan.as_ref().is_some_and(|plan| {
                !normalized_sql.is_empty() && plan.explained_sql.trim() == normalized_sql
            });
            if can_reopen_cached_plan {
                res.show_execution_plan = true;
                handled = true;
            }
        }
    });
    handled
}

pub fn replace_active_tab_sql(
    mut store: TabStore,
    active_tab_id: u64,
    sql: String,
    status: String,
) {
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&active_tab_id) {
            if ed.sql != sql {
                store.result.with_mut(|r| {
                    if let Some(res) = r.get_mut(&active_tab_id) {
                        res.show_execution_plan = false;
                    }
                });
            }
            ed.sql = sql;
        }
    });
    store.result.with_mut(|r| {
        if let Some(res) = r.get_mut(&active_tab_id) {
            res.status = status;
        }
    });
}

pub fn open_structure_tab(mut store: TabStore, session_id: u64, source: TablePreviewSource) {
    let mut next_tab_id = store.next_tab_id;
    let tab_id = next_tab_id();
    next_tab_id += 1;

    let title = format!("Structure · {}", source.table_name);

    let (mut meta, editor, mut result, pending) =
        new_query_tab(tab_id, session_id, title, String::new());
    meta.tab_kind = WorkspaceTabKind::Structure;
    result.status = format!("Loading structure for {}...", source.table_name);
    store.meta.with_mut(|m| {
        m.insert(tab_id, meta);
    });
    store.editor.with_mut(|m| {
        m.insert(tab_id, editor);
    });
    store.result.with_mut(|m| {
        m.insert(tab_id, result);
    });
    store.pending.with_mut(|m| {
        m.insert(tab_id, pending);
    });
    store.active_tab_id.set(tab_id);

    spawn(async move {
        match services::describe_table(session_id, source.schema.clone(), source.table_name.clone())
            .await
        {
            Ok(output) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&tab_id) {
                        res.result = Some(output);
                        res.status = format!("Loaded structure for {}", source.table_name);
                        res.current_offset = 0;
                        res.last_run_sql = None;
                        res.preview_source = None;
                        res.filter = None;
                        res.sort = None;
                        res.is_loading_more = false;
                    }
                });
                store.pending.with_mut(|m| {
                    if let Some(p) = m.get_mut(&tab_id) {
                        p.pending_table_changes = PendingTableChanges::default();
                    }
                });
            }
            Err(err) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&tab_id) {
                        res.result = None;
                        res.status = format!("Structure error: {err}");
                    }
                });
            }
        }
    });
}

pub fn tab_session_or_error(store: TabStore, tab_id: u64, session_id: u64) -> Option<u64> {
    if APP_STATE.read().session(session_id).is_some() {
        Some(session_id)
    } else {
        set_active_tab_status(store, tab_id, "The bound connection was closed".to_string());
        None
    }
}

pub fn maybe_format_sql(
    sql: String,
    auto_format: bool,
    session_id: Option<u64>,
    format_settings: &models::SqlFormatSettings,
) -> String {
    if !auto_format {
        return sql;
    }
    let Some(session_id) = session_id else {
        return sql;
    };
    services::format_sql_for_session(session_id, &sql, format_settings).unwrap_or(sql)
}

fn apply_auto_format_on_run(store: TabStore, current_id: u64, sql: String) -> String {
    let auto_format = APP_EDITOR_BEHAVIOR.peek().auto_format_on_run;
    let format_settings = APP_SQL_FORMAT_SETTINGS.peek().clone();
    if !auto_format {
        return sql;
    }
    let session_id = store.meta.read().get(&current_id).map(|m| m.session_id);
    let formatted = maybe_format_sql(sql.clone(), true, session_id, &format_settings);
    if formatted != sql {
        replace_active_tab_sql(
            store,
            current_id,
            formatted.clone(),
            "SQL formatted".to_string(),
        );
    }
    formatted
}

pub fn run_query_for_tab(
    mut store: TabStore,
    current_id: u64,
    session_id: u64,
    sql: String,
    offset: u64,
    page_size: u32,
    history: Option<QueryHistorySignals>,
) {
    // Dev-only: short-circuit to the mock repo so the empty
    // :memory: pool never has to answer a SQL statement.
    #[cfg(debug_assertions)]
    {
        let tab_session_id = store
            .meta
            .read()
            .get(&current_id)
            .map(|meta| meta.session_id);
        if let Some(session_id) = tab_session_id
            && crate::dev::is_mock_session(session_id)
            && let Some(output) = crate::dev::mock_query_for(&sql)
        {
            let status = match &output {
                QueryOutput::Table(page) => format_loaded_rows_status(page.offset, page.rows.len()),
                QueryOutput::AffectedRows(rows) => format!("Rows affected: {rows}"),
            };
            store.result.with_mut(|m| {
                if let Some(res) = m.get_mut(&current_id) {
                    res.result = Some(output);
                    res.status = status;
                    res.current_offset = 0;
                    res.page_size = page_size;
                    res.last_run_sql = Some(sql.clone());
                    res.preview_source = None;
                    res.is_loading_more = false;
                    res.last_duration_ms = Some(0);
                }
            });
            store.pending.with_mut(|m| {
                if let Some(p) = m.get_mut(&current_id) {
                    p.pending_table_changes = PendingTableChanges::default();
                }
            });
            let _ = history;
            return;
        }
    }
    let sql = apply_auto_format_on_run(store, current_id, sql);
    // Многооператорные скрипты уходят в пакетный исполнитель, который
    // показывает пооператорные результаты. Однооператорные запросы
    // остаются на существующем пути с пагинацией/фильтрами.
    let non_empty_count = services::split_sql(&sql)
        .into_iter()
        .filter(|stmt| !stmt.is_empty())
        .count();
    if non_empty_count > 1 {
        run_batch_for_tab(store, current_id, session_id, sql, page_size, history);
        return;
    }

    if read_only_mode_blocks_sql(&sql) {
        set_active_tab_status(store, current_id, read_only_mode_block_status("write SQL"));
        return;
    }

    let filter = store
        .result
        .read()
        .get(&current_id)
        .and_then(|r| r.filter.clone());
    let sort = store
        .result
        .read()
        .get(&current_id)
        .and_then(|r| r.sort.clone());
    let load_generation = bump_load_generation(store, current_id);

    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            res.status = format!("Running query at offset {offset}...");
            res.preview_source = None;
            res.is_loading_more = false;
            res.show_execution_plan = false;
            res.last_duration_ms = None;
            res.batch_results = None;
            res.batch_outputs.clear();
        }
    });
    store.pending.with_mut(|m| {
        if let Some(p) = m.get_mut(&current_id) {
            p.pending_table_changes = PendingTableChanges::default();
        }
    });

    let connection_type = connection_type_for_session(session_id);

    spawn(async move {
        let start_time = Instant::now();
        match services::execute_query_page(session_id, sql.clone(), page_size, offset, filter, sort)
            .await
        {
            Ok(output) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let duration_suffix =
                    format!(" · {}", super::helpers::format_duration(duration_ms));
                // Подпись результата без суффикса длительности — она хранится
                // отдельно в LastQuerySummary, чтобы статус-бар мог показывать
                // результат и тайминг независимо друг от друга.
                let (status_label, current_offset) = match &output {
                    QueryOutput::Table(page) => (
                        format_loaded_rows_status(page.offset, page.rows.len()),
                        page.offset,
                    ),
                    QueryOutput::AffectedRows(rows) => (format!("Rows affected: {rows}"), 0),
                };
                let status = format!("{status_label}{duration_suffix}");
                let rows_returned = match &output {
                    QueryOutput::Table(page) => Some(page.rows.len()),
                    QueryOutput::AffectedRows(count) => Some(*count as usize),
                };

                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        if res.load_generation != load_generation {
                            return;
                        }
                        res.result = Some(output);
                        res.status = status.clone();
                        res.current_offset = current_offset;
                        res.page_size = page_size;
                        res.last_run_sql = Some(sql.clone());
                        res.preview_source = None;
                        res.is_loading_more = false;
                        res.last_duration_ms = Some(duration_ms);
                    }
                });
                store.pending.with_mut(|m| {
                    if let Some(p) = m.get_mut(&current_id) {
                        p.pending_table_changes = PendingTableChanges::default();
                    }
                });

                set_last_query(Some(LastQuerySummary {
                    label: status_label,
                    duration_ms: Some(duration_ms),
                    failed: false,
                }));

                if let Some((mut history, mut next_history_id, tab_title, connection_name)) =
                    history
                {
                    let history_id = next_history_id();
                    next_history_id += 1;
                    let history_item = QueryHistoryItem {
                        id: history_id,
                        tab_title,
                        connection_name,
                        sql: redact_sql(&sql),
                        duration_ms,
                        rows_returned,
                        executed_at: unix_timestamp(),
                        connection_type: connection_type.clone(),
                        outcome: "Success".to_string(),
                        error_message: None,
                    };
                    history.with_mut(|items| {
                        items.insert(0, history_item.clone());
                        if items.len() > 20 {
                            items.truncate(20);
                        }
                    });
                    let _ = services::append_query_history(history_item).await;
                }
            }
            Err(err) => {
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let duration_suffix =
                    format!(" · {}", super::helpers::format_duration(duration_ms));
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        if res.load_generation != load_generation {
                            return;
                        }
                        res.result = None;
                        res.status = format!("Error: {err}{duration_suffix}");
                        res.preview_source = None;
                        res.is_loading_more = false;
                        res.last_duration_ms = Some(duration_ms);
                    }
                });
                store.pending.with_mut(|m| {
                    if let Some(p) = m.get_mut(&current_id) {
                        p.pending_table_changes = PendingTableChanges::default();
                    }
                });

                set_last_query(Some(LastQuerySummary {
                    label: "Error".to_string(),
                    duration_ms: Some(duration_ms),
                    failed: true,
                }));

                if let Some((mut history, mut next_history_id, tab_title, connection_name)) =
                    history
                {
                    let history_id = next_history_id();
                    next_history_id += 1;
                    let history_item = QueryHistoryItem {
                        id: history_id,
                        tab_title,
                        connection_name,
                        sql: redact_sql(&sql),
                        duration_ms,
                        rows_returned: None,
                        executed_at: unix_timestamp(),
                        connection_type: connection_type.clone(),
                        outcome: format!("Error: {err}"),
                        error_message: Some(err.to_string()),
                    };
                    history.with_mut(|items| {
                        items.insert(0, history_item.clone());
                        if items.len() > 20 {
                            items.truncate(20);
                        }
                    });
                    let _ = services::append_query_history(history_item).await;
                }
            }
        }
    });
}

/// Выполняет многооператорный скрипт пооператорно и собирает результаты
/// в `tab.batch_results` / `tab.batch_outputs` для отображения вкладками.
///
/// Транзакция на стороне сервера НЕ оборачивается: пул соединений выдаёт
/// разные подключения на каждый `execute_query_page`, поэтому `BEGIN`/
/// `COMMIT` через пул не образуют атомарную транзакцию. Каждый оператор
/// выполняется в auto-commit (как в DBeaver с auto-commit). Ручное
/// управление транзакцией — отдельная задача (#10).
///
/// На первой ошибке выполнение останавливается, оставшиеся операторы
/// помечаются `Skipped`.
fn run_batch_for_tab(
    mut store: TabStore,
    current_id: u64,
    session_id: u64,
    sql: String,
    page_size: u32,
    history: Option<QueryHistorySignals>,
) {
    let sql = apply_auto_format_on_run(store, current_id, sql);
    let family = connection_family_for_session(session_id);
    let plan = services::plan_batch(&sql, family);

    if APP_READ_ONLY_MODE() && plan.has_writes {
        set_active_tab_status(store, current_id, read_only_mode_block_status("write SQL"));
        return;
    }
    if plan.executable_count == 0 {
        set_active_tab_status(store, current_id, "Query is empty".to_string());
        return;
    }

    let results: Vec<BatchResult> = plan
        .statements
        .iter()
        .filter(|stmt| !stmt.is_empty())
        .map(|stmt| BatchResult {
            index: stmt.index,
            line: stmt.line,
            preview: preview_statement(&stmt.sql),
            outcome: BatchOutcome::Running,
            duration_ms: None,
            rows: None,
            error_message: None,
        })
        .collect();
    let statement_count = results.len();
    let connection_type = connection_type_for_session(session_id);

    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            res.status = format!("Running batch: {statement_count} statements...");
            res.result = None;
            res.preview_source = None;
            res.is_loading_more = false;
            res.show_execution_plan = false;
            res.last_duration_ms = None;
            res.batch_results = Some(BatchRunState {
                results,
                active_index: 0,
                tx_state: BatchTransactionState::None,
                total_duration_ms: 0,
            });
            res.batch_outputs = vec![None; statement_count];
        }
    });
    store.pending.with_mut(|m| {
        if let Some(p) = m.get_mut(&current_id) {
            p.pending_table_changes = PendingTableChanges::default();
        }
    });

    spawn(async move {
        let batch_start = Instant::now();
        let mut total_ms = 0u64;
        let mut error_pos: Option<usize> = None;
        let mut first_output_pos: Option<usize> = None;

        let mut pos = 0usize;
        for stmt in &plan.statements {
            if stmt.is_empty() {
                continue;
            }
            let stmt_start = Instant::now();
            let executed = services::execute_query_page(
                session_id,
                stmt.sql.clone(),
                page_size,
                0,
                None,
                None,
            )
            .await;
            let duration_ms = stmt_start.elapsed().as_millis() as u64;
            total_ms += duration_ms;

            match executed {
                Ok(output) => {
                    let rows = match &output {
                        QueryOutput::Table(page) => Some(page.rows.len()),
                        QueryOutput::AffectedRows(count) => Some(*count as usize),
                    };
                    // Первый успешный оператор становится активным по умолчанию.
                    first_output_pos.get_or_insert(pos);
                    let output_for_slot = Some(output.clone());
                    store.result.with_mut(|m| {
                        if let Some(res) = m.get_mut(&current_id) {
                            if let Some(batch) = res.batch_results.as_mut() {
                                if let Some(result) = batch.results.get_mut(pos) {
                                    result.outcome = BatchOutcome::Ok;
                                    result.duration_ms = Some(duration_ms);
                                    result.rows = rows;
                                }
                                batch.total_duration_ms = total_ms;
                            }
                            if let Some(slot) = res.batch_outputs.get_mut(pos) {
                                *slot = output_for_slot;
                            }
                        }
                    });
                }
                Err(err) => {
                    let message = err.to_string();
                    store.result.with_mut(|m| {
                        if let Some(res) = m.get_mut(&current_id)
                            && let Some(batch) = res.batch_results.as_mut()
                        {
                            if let Some(result) = batch.results.get_mut(pos) {
                                result.outcome = BatchOutcome::Error;
                                result.duration_ms = Some(duration_ms);
                                result.error_message = Some(message.clone());
                            }
                            batch.total_duration_ms = total_ms;
                        }
                    });
                    error_pos = Some(pos);
                    break;
                }
            }
            pos += 1;
        }

        // Оставшиеся после ошибки операторы — пропущены.
        if let Some(failed_pos) = error_pos {
            store.result.with_mut(|m| {
                if let Some(res) = m.get_mut(&current_id)
                    && let Some(batch) = res.batch_results.as_mut()
                {
                    for result in batch.results.iter_mut().skip(failed_pos + 1) {
                        result.outcome = BatchOutcome::Skipped;
                    }
                }
            });
        }

        let total_duration_ms = batch_start.elapsed().as_millis() as u64;
        let duration_suffix = format!(" · {}", super::helpers::format_duration(total_duration_ms));

        // Финальный статус и синхронизация tab.result с первым табличным
        // результатом (чтобы экспорт/статус-бар работали как обычно).
        let (status_label, failed) = match error_pos {
            Some(failed_pos) => (
                format!(
                    "Batch failed at statement {}/{statement_count}{duration_suffix}",
                    failed_pos + 1
                ),
                true,
            ),
            None => (
                format!("Batch complete: {statement_count} statements{duration_suffix}"),
                false,
            ),
        };

        let active_index = first_output_pos.unwrap_or(0);
        store.result.with_mut(|m| {
            if let Some(res) = m.get_mut(&current_id) {
                res.status = status_label.clone();
                res.last_duration_ms = Some(total_duration_ms);
                res.result = res
                    .batch_outputs
                    .get(active_index)
                    .and_then(|slot| slot.clone());
                if let Some(batch) = res.batch_results.as_mut() {
                    batch.total_duration_ms = total_duration_ms;
                    batch.active_index = active_index;
                }
            }
        });

        set_last_query(Some(LastQuerySummary {
            label: status_label,
            duration_ms: Some(total_duration_ms),
            failed,
        }));

        if let Some((mut history, mut next_history_id, tab_title, connection_name)) = history {
            let history_id = next_history_id();
            next_history_id += 1;
            let rows_returned = {
                let total: usize = store
                    .result
                    .read()
                    .get(&current_id)
                    .and_then(|res| res.batch_results.as_ref())
                    .map(|batch| batch.results.iter().filter_map(|r| r.rows).sum())
                    .unwrap_or(0);
                if total > 0 { Some(total) } else { None }
            };
            let outcome = if failed {
                "Error".to_string()
            } else {
                "Success".to_string()
            };
            let error_message = if failed {
                store
                    .result
                    .read()
                    .get(&current_id)
                    .and_then(|res| res.batch_results.as_ref())
                    .and_then(|batch| batch.results.iter().find_map(|r| r.error_message.clone()))
            } else {
                None
            };
            let history_item = QueryHistoryItem {
                id: history_id,
                tab_title,
                connection_name,
                sql: redact_sql(&sql),
                duration_ms: total_duration_ms,
                rows_returned,
                executed_at: unix_timestamp(),
                connection_type,
                outcome,
                error_message,
            };
            history.with_mut(|items| {
                items.insert(0, history_item.clone());
                if items.len() > 20 {
                    items.truncate(20);
                }
            });
            let _ = services::append_query_history(history_item).await;
        }
    });
}

pub fn run_explain_for_tab(mut store: TabStore, current_id: u64, session_id: u64, sql: String) {
    if !session_capabilities(session_id).is_some_and(can_explain) {
        set_active_tab_status(
            store,
            current_id,
            "Explain Plan is not supported for this connection".to_string(),
        );
        return;
    }
    if sql.trim().is_empty() {
        store.result.with_mut(|m| {
            if let Some(res) = m.get_mut(&current_id) {
                res.status = "Query is empty".to_string();
            }
        });
        return;
    }

    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            res.status = "Running EXPLAIN...".to_string();
            res.execution_plan = None;
        }
    });

    spawn(async move {
        match services::execute_explain(session_id, &sql, false).await {
            Ok(plan) => {
                let node_count = plan.flattened_with_depth().len();
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        res.execution_plan = Some(plan);
                        res.show_execution_plan = true;
                        res.status = format!("Execution plan loaded ({} operations)", node_count);
                    }
                });
            }
            Err(err) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        res.status = format!("EXPLAIN error: {err}");
                    }
                });
            }
        }
    });
}

fn clear_query_chrome_for_preview(res: &mut TabResultState) {
    res.batch_results = None;
    res.batch_outputs.clear();
    res.show_execution_plan = false;
    res.execution_plan = None;
}

fn bump_load_generation(mut store: TabStore, tab_id: u64) -> u64 {
    let mut generation = 0;
    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&tab_id) {
            res.load_generation = res.load_generation.wrapping_add(1);
            generation = res.load_generation;
        }
    });
    generation
}

pub fn run_table_preview_for_tab(
    mut store: TabStore,
    current_id: u64,
    session_id: u64,
    source: TablePreviewSource,
    offset: u64,
    page_size: u32,
) {
    // Opening a table preview should surface the table editor as the
    // primary view. If the SQL editor is currently shown (e.g. the user
    // left it open), hide it so the table's Data/Structure/DDL tabs are
    // not pushed below the editor.
    set_show_sql_editor(false);

    // Dev-only: short-circuit to the mock repo so the empty
    // :memory: pool never has to answer a SQL statement.
    #[cfg(debug_assertions)]
    {
        if let Some(output) = crate::dev::mock_preview_for(&source) {
            let status = match &output {
                QueryOutput::Table(page) => format_loaded_rows_from_source_status(
                    page.offset,
                    page.rows.len(),
                    &source.table_name,
                ),
                QueryOutput::AffectedRows(rows) => format!("Rows affected: {rows}"),
            };
            store.result.with_mut(|m| {
                if let Some(res) = m.get_mut(&current_id) {
                    clear_query_chrome_for_preview(res);
                    res.preview_source = Some(source.clone());
                    res.result = Some(output);
                    res.status = status;
                    res.current_offset = offset;
                    res.page_size = page_size;
                    res.last_run_sql = Some(format!(
                        "select * from {} limit {};",
                        source.qualified_name, page_size
                    ));
                    res.is_loading_more = false;
                }
            });
            store.meta.with_mut(|m| {
                if let Some(meta) = m.get_mut(&current_id) {
                    meta.tab_kind = WorkspaceTabKind::TablePreview;
                    meta.title = source.table_name.clone();
                }
            });
            return;
        }
    }
    let filter = store.result.read().get(&current_id).and_then(|res| {
        if res.preview_source.as_ref() == Some(&source) {
            res.filter.clone()
        } else {
            None
        }
    });
    let sort = store.result.read().get(&current_id).and_then(|res| {
        if res.preview_source.as_ref() == Some(&source) {
            res.sort.clone()
        } else {
            None
        }
    });
    let load_generation = bump_load_generation(store, current_id);

    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            clear_query_chrome_for_preview(res);
            res.status = format!("Loading rows from {}...", source.table_name);
            if res.preview_source.as_ref() != Some(&source) {
                res.filter = None;
                res.sort = None;
                res.is_loading_more = false;
            }
            res.preview_source = Some(source.clone());
        }
    });
    store.pending.with_mut(|m| {
        if let Some(p) = m.get_mut(&current_id) {
            p.pending_table_changes = PendingTableChanges::default();
        }
    });
    store.meta.with_mut(|m| {
        if let Some(meta) = m.get_mut(&current_id) {
            meta.tab_kind = WorkspaceTabKind::TablePreview;
            meta.title = source.table_name.clone();
        }
    });

    spawn(async move {
        match services::load_table_preview_page(
            session_id,
            source.clone(),
            page_size,
            offset,
            filter,
            sort,
        )
        .await
        {
            Ok(output) => {
                let status = match &output {
                    QueryOutput::Table(page) => format_loaded_rows_from_source_status(
                        page.offset,
                        page.rows.len(),
                        &source.table_name,
                    ),
                    QueryOutput::AffectedRows(rows) => format!("Rows affected: {rows}"),
                };

                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        if res.load_generation != load_generation {
                            return;
                        }
                        clear_query_chrome_for_preview(res);
                        res.result = Some(output);
                        res.status = status;
                        res.current_offset = offset;
                        res.page_size = page_size;
                        res.last_run_sql = Some(format!(
                            "select * from {} limit {};",
                            source.qualified_name, page_size
                        ));
                        res.preview_source = Some(source.clone());
                        res.is_loading_more = false;
                    }
                });
            }
            Err(err) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_id) {
                        if res.load_generation != load_generation {
                            return;
                        }
                        res.result = None;
                        res.status = format!("Preview error: {err}");
                        res.preview_source = Some(source.clone());
                        res.is_loading_more = false;
                    }
                });
            }
        }
    });
}

/// Maximum number of rows that can accumulate via infinite-scroll append.
/// Beyond this cap the user must use explicit pagination (Previous/Next) instead.
const MAX_ACCUMULATED_ROWS: usize = 10_000;

fn append_query_page(existing_page: &mut models::QueryPage, next_page: models::QueryPage) {
    let next_editable = next_page.editable;

    existing_page.rows.extend(next_page.rows);
    existing_page.has_next = next_page.has_next;
    existing_page.has_previous = existing_page.has_previous || next_page.has_previous;

    existing_page.editable = match (existing_page.editable.take(), next_editable) {
        (Some(mut existing_editable), Some(next_editable))
            if existing_editable.source == next_editable.source =>
        {
            existing_editable
                .row_locators
                .extend(next_editable.row_locators);
            Some(existing_editable)
        }
        (None, None) => None,
        _ => None,
    };

    // Cap accumulated rows to prevent unbounded memory growth and DOM freeze.
    if existing_page.rows.len() > MAX_ACCUMULATED_ROWS {
        let excess = existing_page.rows.len() - MAX_ACCUMULATED_ROWS;
        existing_page.rows.drain(..excess);
        existing_page.offset += excess as u64;
        if let Some(editable) = existing_page.editable.as_mut() {
            if editable.row_locators.len() >= excess {
                editable.row_locators.drain(..excess);
            } else {
                existing_page.editable = None;
            }
        }
    }

    if existing_page
        .editable
        .as_ref()
        .is_some_and(|editable| editable.row_locators.len() != existing_page.rows.len())
    {
        existing_page.editable = None;
    }
}

pub fn append_next_tab_page(mut store: TabStore, current_tab: QueryTabState) {
    let Some(QueryOutput::Table(current_page)) = current_tab.result.clone() else {
        return;
    };

    if current_tab.is_loading_more || !current_tab.pending_table_changes.is_empty() {
        return;
    }

    if !current_page.has_next {
        return;
    }

    let next_offset = current_page.offset + current_page.rows.len() as u64;
    let expected_sql = current_tab.last_run_sql.clone();
    let expected_preview_source = current_tab.preview_source.clone();
    let expected_filter = current_tab.filter.clone();
    let expected_sort = current_tab.sort.clone();

    let Some(session_id) = tab_session_or_error(store, current_tab.id, current_tab.session_id)
    else {
        return;
    };

    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_tab.id) {
            res.is_loading_more = true;
            res.status = format!("Loading more rows from {}...", next_offset + 1);
        }
    });

    spawn(async move {
        let next_page_result = if let Some(source) = expected_preview_source.clone() {
            services::load_table_preview_page(
                session_id,
                source,
                current_tab.page_size,
                next_offset,
                expected_filter.clone(),
                expected_sort.clone(),
            )
            .await
        } else if let Some(sql) = expected_sql.clone() {
            services::execute_query_page(
                session_id,
                sql,
                current_tab.page_size,
                next_offset,
                expected_filter.clone(),
                expected_sort.clone(),
            )
            .await
        } else {
            store.result.with_mut(|m| {
                if let Some(res) = m.get_mut(&current_tab.id) {
                    res.is_loading_more = false;
                }
            });
            return;
        };

        match next_page_result {
            Ok(QueryOutput::Table(next_page)) => {
                store.result.with_mut(|m| {
                    let Some(res) = m.get_mut(&current_tab.id) else {
                        return;
                    };

                    let same_request = res.last_run_sql == expected_sql
                        && res.preview_source == expected_preview_source
                        && res.filter == expected_filter
                        && res.sort == expected_sort;

                    if !same_request {
                        res.is_loading_more = false;
                        return;
                    }

                    let mut loaded_range = None;
                    if let Some(QueryOutput::Table(existing_page)) = res.result.as_mut() {
                        append_query_page(existing_page, next_page);
                        loaded_range = Some((
                            existing_page.offset,
                            existing_page.offset + existing_page.rows.len() as u64,
                        ));
                    }

                    if let Some((offset, last_row)) = loaded_range {
                        res.current_offset = offset;
                        res.status = format_loaded_rows_status(
                            offset,
                            last_row.saturating_sub(offset) as usize,
                        );
                    }

                    res.is_loading_more = false;
                });
            }
            Ok(other_output) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_tab.id) {
                        res.result = Some(other_output);
                        res.is_loading_more = false;
                        res.status = "Loaded additional result".to_string();
                    }
                });
            }
            Err(err) => {
                store.result.with_mut(|m| {
                    if let Some(res) = m.get_mut(&current_tab.id) {
                        res.is_loading_more = false;
                        res.status = format!("Load more error: {err}");
                    }
                });
            }
        }
    });
}

fn loaded_rows_range(offset: u64, row_count: usize) -> Option<(u64, u64)> {
    if row_count == 0 {
        None
    } else {
        Some((offset + 1, offset + row_count as u64))
    }
}

fn format_loaded_rows_status(offset: u64, row_count: usize) -> String {
    match loaded_rows_range(offset, row_count) {
        Some((start, end)) => format!("Loaded rows {start}-{end}"),
        None => "Loaded 0 rows".to_string(),
    }
}

fn format_loaded_rows_from_source_status(
    offset: u64,
    row_count: usize,
    source_name: &str,
) -> String {
    match loaded_rows_range(offset, row_count) {
        Some((start, end)) => format!("Loaded rows {start}-{end} from {source_name}"),
        None => format!("Loaded 0 rows from {source_name}"),
    }
}

pub(crate) fn rows_toolbar_summary(offset: u64, row_count: usize, page_size: u32) -> String {
    match loaded_rows_range(offset, row_count) {
        Some((start, end)) => format!("Rows {start}-{end} · page size {page_size}"),
        None => format!("0 rows · page size {page_size}"),
    }
}

pub fn load_tab_page(store: TabStore, current_tab: QueryTabState, offset: u64) {
    let Some(session_id) = tab_session_or_error(store, current_tab.id, current_tab.session_id)
    else {
        return;
    };

    if let Some(source) = current_tab.preview_source.clone() {
        run_table_preview_for_tab(
            store,
            current_tab.id,
            session_id,
            source,
            offset,
            current_tab.page_size,
        );
        return;
    }

    if let Some(sql) = current_tab.last_run_sql.clone() {
        run_query_for_tab(
            store,
            current_tab.id,
            session_id,
            sql,
            offset,
            current_tab.page_size,
            None,
        );
    }
}

pub fn refresh_tab_result(
    store: TabStore,
    current_tab: QueryTabState,
    fallback_source: Option<TablePreviewSource>,
) {
    if current_tab.preview_source.is_some() || current_tab.last_run_sql.is_some() {
        load_tab_page(store, current_tab.clone(), current_tab.current_offset);
        return;
    }

    let Some(session_id) = tab_session_or_error(store, current_tab.id, current_tab.session_id)
    else {
        return;
    };

    if let Some(source) = fallback_source {
        run_table_preview_for_tab(
            store,
            current_tab.id,
            session_id,
            source,
            current_tab.current_offset,
            current_tab.page_size,
        );
    }
}

pub fn mark_table_deleted(mut store: TabStore, session_id: u64, source: TablePreviewSource) {
    let tab_ids: Vec<u64> = store
        .meta
        .read()
        .iter()
        .filter(|(_, meta)| meta.session_id == session_id)
        .map(|(id, _)| *id)
        .collect();

    for tab_id in tab_ids {
        let matches_preview = store
            .result
            .read()
            .get(&tab_id)
            .and_then(|r| r.preview_source.clone())
            .as_ref()
            == Some(&source);
        let matches_sql = store
            .result
            .read()
            .get(&tab_id)
            .and_then(|r| r.last_run_sql.clone())
            .and_then(|sql| services::preview_source_for_sql(&sql))
            .as_ref()
            == Some(&source);

        if !matches_preview && !matches_sql {
            continue;
        }

        store.result.with_mut(|m| {
            if let Some(res) = m.get_mut(&tab_id) {
                res.result = None;
                res.current_offset = 0;
                res.preview_source = None;
                res.filter = None;
                res.sort = None;
                res.is_loading_more = false;
                res.status = if matches_preview {
                    format!("Table {} was deleted", source.table_name)
                } else {
                    format!(
                        "Referenced table {} was deleted. Update the SQL and run it again.",
                        source.table_name
                    )
                };

                if matches_preview {
                    res.last_run_sql = None;
                }
            }
        });
        store.pending.with_mut(|m| {
            if let Some(p) = m.get_mut(&tab_id) {
                p.pending_table_changes = PendingTableChanges::default();
            }
        });
    }
}

pub fn mark_table_truncated(mut store: TabStore, session_id: u64, source: TablePreviewSource) {
    let mut preview_tabs = Vec::new();

    let tab_ids: Vec<u64> = store
        .meta
        .read()
        .iter()
        .filter(|(_, meta)| meta.session_id == session_id)
        .map(|(id, _)| *id)
        .collect();

    for tab_id in tab_ids {
        let matches_preview = store
            .result
            .read()
            .get(&tab_id)
            .and_then(|r| r.preview_source.clone())
            .as_ref()
            == Some(&source);
        let matches_sql = store
            .result
            .read()
            .get(&tab_id)
            .and_then(|r| r.last_run_sql.clone())
            .and_then(|sql| services::preview_source_for_sql(&sql))
            .as_ref()
            == Some(&source);

        if !matches_preview && !matches_sql {
            continue;
        }

        store.result.with_mut(|m| {
            if let Some(res) = m.get_mut(&tab_id) {
                res.result = None;
                res.current_offset = 0;
                res.is_loading_more = false;
            }
        });
        store.pending.with_mut(|m| {
            if let Some(p) = m.get_mut(&tab_id) {
                p.pending_table_changes = PendingTableChanges::default();
            }
        });

        if matches_preview {
            let page_size = store.result.read().get(&tab_id).map(|r| r.page_size);
            if let Some(page_size) = page_size {
                preview_tabs.push((tab_id, page_size));
            }
            continue;
        }

        store.result.with_mut(|m| {
            if let Some(res) = m.get_mut(&tab_id) {
                res.filter = None;
                res.sort = None;
                res.status = format!(
                    "Referenced table {} was truncated. Run the SQL again to refresh.",
                    source.table_name
                );
            }
        });
    }

    for (tab_id, page_size) in preview_tabs {
        run_table_preview_for_tab(store, tab_id, session_id, source.clone(), 0, page_size);
    }
}

pub fn toggle_active_tab_sort(mut store: TabStore, active_tab_id: u64, column_name: String) {
    let mut tab_to_reload = None;

    store.result.with_mut(|m| {
        let Some(res) = m.get_mut(&active_tab_id) else {
            return;
        };

        res.sort = next_sort_state(res.sort.as_ref(), &column_name);
        res.current_offset = 0;
        res.status = match &res.sort {
            Some(sort) => format!(
                "Sorted by {} {}",
                sort.column_name,
                if sort.descending { "DESC" } else { "ASC" }
            ),
            None => "Sorting cleared".to_string(),
        };
        tab_to_reload = Some(res.clone());
    });

    if let Some(res) = tab_to_reload
        && (res.last_run_sql.is_some() || res.preview_source.is_some())
    {
        let session_id = store
            .meta
            .read()
            .get(&active_tab_id)
            .map(|meta| meta.session_id)
            .unwrap_or(0);
        let tab = QueryTabState {
            id: active_tab_id,
            session_id,
            page_size: res.page_size,
            current_offset: res.current_offset,
            preview_source: res.preview_source.clone(),
            last_run_sql: res.last_run_sql.clone(),
            ..QueryTabState::default()
        };
        load_tab_page(store, tab, 0);
    }
}

fn next_sort_state(current: Option<&QuerySort>, column_name: &str) -> Option<QuerySort> {
    match current {
        Some(sort) if sort.column_name == column_name && !sort.descending => Some(QuerySort {
            column_name: column_name.to_string(),
            descending: true,
        }),
        Some(sort) if sort.column_name == column_name && sort.descending => None,
        _ => Some(QuerySort {
            column_name: column_name.to_string(),
            descending: false,
        }),
    }
}

pub fn apply_active_tab_filter(mut store: TabStore, active_tab_id: u64, filter: QueryFilter) {
    let mut tab_to_reload = None;

    store.result.with_mut(|m| {
        let Some(res) = m.get_mut(&active_tab_id) else {
            return;
        };

        let applied_rules = filter
            .rules
            .iter()
            .filter(|rule| {
                !rule.column_name.trim().is_empty()
                    && (!rule.value.trim().is_empty() || rule.operator.is_nullary())
            })
            .cloned()
            .collect::<Vec<_>>();

        res.filter = if applied_rules.is_empty() {
            None
        } else {
            Some(QueryFilter {
                mode: filter.mode,
                rules: applied_rules,
            })
        };
        res.current_offset = 0;
        res.status = match &res.filter {
            Some(filter) => format!(
                "Applied {} filter rule(s) with {}",
                filter.rules.len(),
                match filter.mode {
                    QueryFilterMode::And => "AND",
                    QueryFilterMode::Or => "OR",
                }
            ),
            None => "Filter cleared".to_string(),
        };
        tab_to_reload = Some(res.clone());
    });

    if let Some(res) = tab_to_reload
        && (res.last_run_sql.is_some() || res.preview_source.is_some())
    {
        let session_id = store
            .meta
            .read()
            .get(&active_tab_id)
            .map(|meta| meta.session_id)
            .unwrap_or(0);
        let tab = QueryTabState {
            id: active_tab_id,
            session_id,
            page_size: res.page_size,
            current_offset: res.current_offset,
            preview_source: res.preview_source.clone(),
            last_run_sql: res.last_run_sql.clone(),
            ..QueryTabState::default()
        };
        load_tab_page(store, tab, 0);
    }
}

pub fn clear_active_tab_filter(mut store: TabStore, active_tab_id: u64) {
    let mut tab_to_reload = None;

    store.result.with_mut(|m| {
        let Some(res) = m.get_mut(&active_tab_id) else {
            return;
        };

        res.filter = None;
        res.current_offset = 0;
        res.status = "Filter cleared".to_string();
        tab_to_reload = Some(res.clone());
    });

    if let Some(res) = tab_to_reload
        && (res.last_run_sql.is_some() || res.preview_source.is_some())
    {
        let session_id = store
            .meta
            .read()
            .get(&active_tab_id)
            .map(|meta| meta.session_id)
            .unwrap_or(0);
        let tab = QueryTabState {
            id: active_tab_id,
            session_id,
            page_size: res.page_size,
            current_offset: res.current_offset,
            preview_source: res.preview_source.clone(),
            last_run_sql: res.last_run_sql.clone(),
            ..QueryTabState::default()
        };
        load_tab_page(store, tab, 0);
    }
}

// ─────────────────── SQL editor helpers ───────────────────
//
// These wrappers consolidate the per-action logic that the SQL
// editor's context menu and keyboard shortcuts need. The same code
// paths were previously only in `tabs.rs` (the toolbar), which made
// it impossible to expose them via right-click or hotkeys without
// duplicating the connection lookup, history plumbing, and SQL
// trimming boilerplate.

/// Run the SQL in the active tab. Mirrors the toolbar's Run button:
/// trims, checks for empty input, looks up the session connection,
/// and dispatches to `run_query_for_tab` with the configured page
/// size and history sink.
///
/// `history` and `next_history_id` are passed as a tuple because
/// every callsite already has them paired in local bindings; this
/// keeps the API ergonomic without forcing a new struct type.
pub fn run_active_tab(
    store: TabStore,
    active_tab_id: u64,
    history: (Signal<Vec<QueryHistoryItem>>, Signal<u64>),
) {
    let (history, next_history_id) = history;
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return;
    };
    let sql = sql.trim().to_string();
    let tab_title = store.meta.read().get(&current_id).map(|m| m.title.clone());
    let Some(tab_title) = tab_title else {
        return;
    };
    let page_size = store.result.read().get(&current_id).map(|r| r.page_size);
    let Some(page_size) = page_size else {
        return;
    };
    let session_id = store.meta.read().get(&current_id).map(|m| m.session_id);
    let Some(session_id) = session_id else {
        return;
    };
    let connection_name = APP_STATE
        .read()
        .session(session_id)
        .map(|session| session.name.clone())
        .unwrap_or_else(|| "Detached session".to_string());

    if sql.is_empty() {
        set_active_tab_status(store, current_id, "Query is empty".to_string());
        return;
    }

    let Some(session_id) = tab_session_or_error(store, current_id, session_id) else {
        return;
    };

    run_query_for_tab(
        store,
        current_id,
        session_id,
        sql,
        0,
        page_size,
        Some((history, next_history_id, tab_title, connection_name)),
    );
}

/// Run EXPLAIN for the active tab's SQL. Mirrors the toolbar's
/// Explain button.
pub fn run_active_tab_explain(store: TabStore, active_tab_id: u64) {
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return;
    };
    let sql = sql.trim().to_string();
    if toggle_execution_plan_for_tab(store, current_id, &sql) {
        return;
    }
    if sql.is_empty() {
        set_active_tab_status(store, current_id, "Enter a query to explain".to_string());
        return;
    }
    if !services::is_read_only_sql(&sql) {
        set_active_tab_status(
            store,
            current_id,
            "EXPLAIN is only available for read-only SQL".to_string(),
        );
        return;
    }
    let session_id = store.meta.read().get(&current_id).map(|m| m.session_id);
    let Some(session_id) = session_id else {
        return;
    };
    let Some(session_id) = tab_session_or_error(store, current_id, session_id) else {
        return;
    };
    run_explain_for_tab(store, current_id, session_id, sql);
}

/// Format the active tab's SQL in place. Mirrors the toolbar's
/// Format button. The formatter runs on a blocking thread so a large
/// query never stalls the render loop.
pub fn format_active_tab(
    store: TabStore,
    active_tab_id: u64,
    format_settings: models::SqlFormatSettings,
) {
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return;
    };
    let sql = sql.trim();
    if sql.is_empty() {
        set_active_tab_status(
            store,
            current_id,
            "Nothing to format in the current tab".to_string(),
        );
        return;
    }

    let Some(session_id) = store.meta.read().get(&current_id).map(|m| m.session_id) else {
        return;
    };
    let sql = sql.to_string();
    let fallback_sql = sql.clone();
    spawn(async move {
        let formatted = tokio::task::spawn_blocking(move || {
            services::format_sql_for_session(session_id, &sql, &format_settings).unwrap_or(sql)
        })
        .await
        .unwrap_or(fallback_sql);
        replace_active_tab_sql(store, current_id, formatted, "SQL formatted".to_string());
    });
}

/// Toggle `--` line comments on every selected line in the active
/// tab. Operates on byte offsets in the SQL text, expanding a
/// collapsed cursor to its containing line range first so the
/// shortcut works whether the user has a selection or not.
///
/// Returns `true` if the SQL was modified.
pub fn toggle_line_comments_in_active_tab(
    mut store: TabStore,
    active_tab_id: u64,
    selection: std::ops::Range<usize>,
) -> bool {
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return false;
    };
    if sql.is_empty() {
        return false;
    }

    // Clamp selection to the SQL bounds and expand to whole lines.
    let len = sql.len();
    let start = selection.start.min(len);
    let end = selection.end.min(len);
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };

    // Snap to the nearest char boundaries (cursor offsets come from
    // JS in UTF-16 units, but we run byte math here — walking
    // backwards is safe because we only ever land on boundaries).
    let mut line_start = start;
    while line_start > 0 && !sql.is_char_boundary(line_start) {
        line_start -= 1;
    }
    let mut line_end = end;
    while line_end > 0 && !sql.is_char_boundary(line_end) {
        line_end -= 1;
    }

    // Expand to the start of `line_start`'s line and the end of
    // `line_end`'s line.
    let expanded_start = sql[..line_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let expanded_end = sql[line_end..]
        .find('\n')
        .map(|p| line_end + p)
        .unwrap_or(len);

    let segment = &sql[expanded_start..expanded_end];
    let lines: Vec<&str> = segment.split('\n').collect();
    if lines.is_empty() {
        return false;
    }

    // If every non-empty line in the range already starts with
    // `--`, uncomment them. Otherwise comment every line.
    let non_empty_lines: Vec<&&str> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let all_commented = !non_empty_lines.is_empty()
        && non_empty_lines
            .iter()
            .all(|line| line.trim_start().starts_with("--"));

    let new_segment = if all_commented {
        uncomment_segment(segment)
    } else {
        comment_segment(segment, 0..0)
    };

    let mut new_sql = String::with_capacity(sql.len() + 8);
    new_sql.push_str(&sql[..expanded_start]);
    new_sql.push_str(&new_segment);
    new_sql.push_str(&sql[expanded_end..]);

    if new_sql == sql {
        return false;
    }

    let new_cursor = if expanded_end > 0 { expanded_start } else { 0 };
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&current_id) {
            ed.sql = new_sql;
        }
    });
    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            res.status = if all_commented {
                "Uncommented selection".to_string()
            } else {
                "Commented selection".to_string()
            };
            res.show_execution_plan = false;
        }
    });
    let _ = new_cursor; // cursor reset handled by the editor's sync effect
    true
}

/// Remove a leading `-- ` (or `--`) from a single line. If the line
/// is empty or does not start with `--`, it is returned unchanged.
fn strip_line_comment(line: &str) -> String {
    let trimmed_start = line.trim_start();
    if !trimmed_start.starts_with("--") {
        return line.to_string();
    }
    let leading_ws_len = line.len() - trimmed_start.len();
    let after_dashes = &trimmed_start[2..];
    let after = after_dashes.strip_prefix(' ').unwrap_or(after_dashes);
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..leading_ws_len]);
    out.push_str(after);
    out
}

/// Persist the active tab's current SQL as a saved query, with the
/// tab's title as the default name. Returns a user-visible status
/// string suitable for a toast.
pub fn save_active_tab_as_saved_query(
    store: TabStore,
    active_tab_id: u64,
    mut saved_queries_signal: Signal<Vec<models::SavedQuery>>,
    mut next_saved_query_id: Signal<u64>,
) -> String {
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return "No active SQL tab available.".to_string();
    };
    if sql.trim().is_empty() {
        return "Current SQL tab is empty.".to_string();
    }
    let title = store.meta.read().get(&current_id).map(|m| m.title.clone());
    let Some(title) = title else {
        return "No active SQL tab available.".to_string();
    };
    let session_id = store.meta.read().get(&current_id).map(|m| m.session_id);
    let connection_name = session_id.and_then(|sid| APP_STATE.read().session_name(sid));
    let item = models::SavedQuery {
        id: next_saved_query_id(),
        title,
        folder: String::new(),
        sql,
        kind: models::SavedQueryKind::Query,
        connection_name,
    };
    let title = item.title.clone();
    saved_queries_signal.with_mut(|items| {
        items.push(item.clone());
        items.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
    });
    let id = item.id;
    let item_for_storage = item;
    spawn(async move {
        let _ = services::save_saved_query(item_for_storage).await;
    });
    // Mark the new id as taken so the caller can keep counting.
    if id + 1 > *next_saved_query_id.peek() {
        next_saved_query_id.set(id + 1);
    }
    format!("Saved {}.", title)
}

/// Clear the active tab's SQL (without deleting the tab itself).
pub fn clear_active_tab_sql(store: TabStore, active_tab_id: u64) {
    replace_active_tab_sql(store, active_tab_id, String::new(), "Cleared".to_string());
}

/// Indent / outdent every line in the active tab's selection by
/// two spaces. Mirrors a basic editor experience.
pub fn indent_lines_in_active_tab(
    mut store: TabStore,
    active_tab_id: u64,
    selection: std::ops::Range<usize>,
    direction: IndentDirection,
) {
    let current_id = active_tab_id;
    let sql = store
        .editor
        .read()
        .get(&current_id)
        .map(|ed| ed.sql.clone());
    let Some(sql) = sql else {
        return;
    };
    let len = sql.len();
    let start = selection.start.min(len);
    let end = selection.end.min(len);
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let expanded_start = sql[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let expanded_end = sql[end..].find('\n').map(|p| end + p).unwrap_or(len);
    let segment = &sql[expanded_start..expanded_end];
    let new_segment = indent_segment(segment, direction);
    let new_sql = format!(
        "{}{}{}",
        &sql[..expanded_start],
        new_segment,
        &sql[expanded_end..]
    );
    if new_sql == sql {
        return;
    }
    store.editor.with_mut(|m| {
        if let Some(ed) = m.get_mut(&current_id) {
            ed.sql = new_sql;
        }
    });
    store.result.with_mut(|m| {
        if let Some(res) = m.get_mut(&current_id) {
            res.status = match direction {
                IndentDirection::In => "Indented".to_string(),
                IndentDirection::Out => "Outdented".to_string(),
            };
            res.show_execution_plan = false;
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentDirection {
    In,
    Out,
}

fn apply_indent(line: &str, direction: IndentDirection) -> String {
    // Empty / whitespace-only lines are kept verbatim: indenting
    // a blank line by two spaces would be invisible to the user
    // but still affect the byte count of the SQL, so we skip them
    // entirely.
    if line.chars().all(|c| c.is_whitespace()) {
        return line.to_string();
    }
    match direction {
        IndentDirection::In => format!("  {}", line),
        IndentDirection::Out => {
            let leading_ws_len = line
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(|c| c.len_utf8())
                .sum::<usize>();
            let to_strip = leading_ws_len.min(2);
            if to_strip == 0 {
                line.to_string()
            } else {
                let mut out = String::with_capacity(line.len());
                let mut consumed = 0;
                for c in line.chars() {
                    if consumed < to_strip && c.is_whitespace() {
                        consumed += c.len_utf8();
                        continue;
                    }
                    out.push(c);
                }
                out
            }
        }
    }
}

// ─────────────────── Pure segment transforms ───────────────────
//
// These functions operate on a `&str` segment of SQL without any
// Dioxus state. They power both the runtime-bound Signal helpers
// (see the public `toggle_line_comments_in_active_tab` and
// `indent_lines_in_active_tab`) and the unit tests above. Keeping
// the transform pure makes the algorithms testable without a
// runtime and reusable from outside the workspace context.

pub(crate) fn comment_segment(sql: &str, _selection: std::ops::Range<usize>) -> String {
    sql.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else {
                format!("-- {}", line.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn uncomment_segment(sql: &str) -> String {
    sql.split('\n')
        .map(strip_line_comment)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn indent_segment(sql: &str, direction: IndentDirection) -> String {
    sql.split('\n')
        .map(|line| apply_indent(line, direction))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validate that a rewritten SQL is safe to insert (read-only).
pub fn apply_optimized_sql_impl(sql: &str) -> Result<(), String> {
    if services::is_read_only_sql(sql) {
        Ok(())
    } else {
        Err("The optimized SQL is not read-only; refusing to insert.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IndentDirection,
        PreviewTabCandidate,
        append_query_page,
        apply_indent,
        apply_optimized_sql_impl,
        comment_segment,
        format_loaded_rows_from_source_status,
        format_loaded_rows_status,
        indent_segment,
        maybe_format_sql,
        preview_statement,
        redact_sql,
        resolve_table_preview_tab_id,
        rows_toolbar_summary,
        strip_line_comment,
        sync_tab_sql_draft,
        toggle_cached_execution_plan,
        uncomment_segment,
    };

    use models::{
        EditableTableContext,
        ExecutionPlan,
        QueryPage,
        QueryTabState,
        TablePreviewSource,
        WorkspaceTabKind,
    };

    fn query_tab(sql: &str) -> QueryTabState {
        QueryTabState {
            id: 1,
            session_id: 7,
            title: "Query 1".to_string(),
            sql: sql.to_string(),
            status: "Ready".to_string(),
            page_size: 100,
            tab_kind: WorkspaceTabKind::Query,
            ..QueryTabState::default()
        }
    }

    fn test_source() -> TablePreviewSource {
        TablePreviewSource {
            schema: None,
            table_name: "products".to_string(),
            qualified_name: "products".to_string(),
        }
    }

    fn query_page(offset: u64, row_count: usize, has_next: bool) -> QueryPage {
        let rows = (0..row_count)
            .map(|index| vec![(offset + index as u64).to_string()])
            .collect::<Vec<_>>();
        let row_locators = (0..row_count)
            .map(|index| format!("row-{}", offset + index as u64))
            .collect::<Vec<_>>();

        QueryPage {
            columns: vec!["id".to_string()],
            rows,
            editable: Some(EditableTableContext {
                source: test_source(),
                row_locators,
            }),
            offset,
            page_size: row_count as u32,
            has_previous: offset > 0,
            has_next,
        }
    }

    #[test]
    fn maybe_format_sql_is_noop_when_disabled() {
        let sql = "select 1";
        let out = maybe_format_sql(
            sql.into(),
            false,
            None,
            &models::SqlFormatSettings::default(),
        );
        assert_eq!(out, "select 1");
    }

    #[test]
    fn preview_tab_reuses_existing_table_preview() {
        let tabs = vec![
            PreviewTabCandidate {
                id: 1,
                session_id: 7,
                tab_kind: WorkspaceTabKind::Query,
                preview_qualified_name: None,
            },
            PreviewTabCandidate {
                id: 2,
                session_id: 7,
                tab_kind: WorkspaceTabKind::TablePreview,
                preview_qualified_name: Some("employees".to_string()),
            },
        ];
        assert_eq!(resolve_table_preview_tab_id(&tabs, 7, "employees"), Some(2));
    }

    #[test]
    fn preview_tab_falls_back_to_query_tab_for_session() {
        let tabs = vec![PreviewTabCandidate {
            id: 1,
            session_id: 7,
            tab_kind: WorkspaceTabKind::Query,
            preview_qualified_name: None,
        }];
        assert_eq!(resolve_table_preview_tab_id(&tabs, 7, "employees"), Some(1));
    }

    #[test]
    fn preview_tab_does_not_reuse_other_session() {
        let tabs = vec![PreviewTabCandidate {
            id: 1,
            session_id: 3,
            tab_kind: WorkspaceTabKind::TablePreview,
            preview_qualified_name: Some("employees".to_string()),
        }];
        assert_eq!(resolve_table_preview_tab_id(&tabs, 7, "employees"), None);
    }

    #[test]
    fn formats_empty_result_status_without_invalid_range() {
        assert_eq!(format_loaded_rows_status(0, 0), "Loaded 0 rows");
        assert_eq!(
            format_loaded_rows_from_source_status(0, 0, "products"),
            "Loaded 0 rows from products"
        );
    }

    #[test]
    fn formats_empty_result_toolbar_summary_without_invalid_range() {
        assert_eq!(rows_toolbar_summary(0, 0, 100), "0 rows · page size 100");
    }

    #[test]
    fn second_explain_click_hides_visible_execution_plan() {
        let mut tab = query_tab("select 1");
        tab.execution_plan = Some(ExecutionPlan::new("select 1"));
        tab.show_execution_plan = true;

        assert!(toggle_cached_execution_plan(&mut tab, "select 1"));
        assert!(!tab.show_execution_plan);
    }

    #[test]
    fn explain_click_reopens_cached_plan_for_same_sql() {
        let mut tab = query_tab("select 1");
        tab.execution_plan = Some(ExecutionPlan::new("select 1"));
        tab.show_execution_plan = false;

        assert!(toggle_cached_execution_plan(&mut tab, "select 1"));
        assert!(tab.show_execution_plan);
    }

    #[test]
    fn explain_click_does_not_reopen_cached_plan_for_different_sql() {
        let mut tab = query_tab("select 1");
        tab.execution_plan = Some(ExecutionPlan::new("select 1"));
        tab.show_execution_plan = false;

        assert!(!toggle_cached_execution_plan(&mut tab, "select 2"));
        assert!(!tab.show_execution_plan);
    }

    #[test]
    fn syncing_editor_draft_updates_sql_and_hides_plan_without_resetting_result_state() {
        let mut tab = query_tab("select 1");
        tab.execution_plan = Some(ExecutionPlan::new("select 1"));
        tab.show_execution_plan = true;
        tab.status = "Loaded 1 rows".to_string();

        sync_tab_sql_draft(&mut tab, "select 2");

        assert_eq!(tab.sql, "select 2");
        assert!(!tab.show_execution_plan);
        assert_eq!(tab.status, "Loaded 1 rows");
    }

    #[test]
    fn append_query_page_caps_rows_and_keeps_edit_locators_aligned() {
        let mut existing = query_page(0, 100, true);
        let next = query_page(100, 11_000, false);

        append_query_page(&mut existing, next);

        assert_eq!(existing.rows.len(), 10_000);
        assert_eq!(existing.offset, 1_100);
        assert_eq!(existing.rows.first().unwrap()[0], "1100");
        assert_eq!(existing.rows.last().unwrap()[0], "11099");
        assert_eq!(
            existing.editable.as_ref().unwrap().row_locators.len(),
            10_000
        );
        assert_eq!(
            existing.editable.as_ref().unwrap().row_locators.first(),
            Some(&"row-1100".to_string())
        );
        assert!(!existing.has_next);
    }

    #[test]
    fn redacts_unquoted_secret_values_without_leaking_prefix() {
        let sql = "set password=abc123;\nselect 1;";

        let redacted = redact_sql(sql);

        assert_eq!(redacted, "set password= [REDACTED]\nselect 1;");
        assert!(!redacted.contains("abc123"));
    }

    #[test]
    fn redacts_quoted_secret_values_without_unwrapping_quote() {
        let sql = "alter user app with password = 'abc123';";

        let redacted = redact_sql(sql);

        assert_eq!(redacted, "alter user app with password = [REDACTED]");
        assert!(!redacted.contains("abc123"));
    }

    // ─────────────────── SQL editor helper tests ───────────────────
    //
    // We deliberately test only the pure, runtime-free helpers
    // here. The Signal-bound entry points (`toggle_line_comments_in_
    // active_tab`, `clear_active_tab_sql`) are exercised end-to-end
    // through the workspace screen in the integration smoke test in
    // `screens::workspace::components::sql_editor`.

    #[test]
    fn strip_line_comment_handles_dash_dash_with_space() {
        assert_eq!(strip_line_comment("-- foo"), "foo");
        assert_eq!(strip_line_comment("  -- bar"), "  bar");
        assert_eq!(strip_line_comment("--baz"), "baz");
    }

    #[test]
    fn strip_line_comment_passthrough_when_no_comment() {
        assert_eq!(strip_line_comment("select 1"), "select 1");
        assert_eq!(strip_line_comment(""), "");
        assert_eq!(strip_line_comment("   "), "   ");
        // `--` not at the start (after whitespace) is still stripped
        // because the rule is "after leading whitespace". Other
        // dash patterns are passed through.
        assert_eq!(strip_line_comment("a -- inline"), "a -- inline");
    }

    #[test]
    fn apply_indent_in_prepends_two_spaces() {
        assert_eq!(apply_indent("select 1", IndentDirection::In), "  select 1");
        assert_eq!(
            apply_indent("  select 1", IndentDirection::In),
            "    select 1"
        );
    }

    #[test]
    fn apply_indent_out_strips_up_to_two_spaces() {
        assert_eq!(
            apply_indent("    select 1", IndentDirection::Out),
            "  select 1"
        );
        assert_eq!(apply_indent("  select 1", IndentDirection::Out), "select 1");
        assert_eq!(apply_indent("select 1", IndentDirection::Out), "select 1");
        // Tabs count as whitespace too; we strip up to 2 byte-units.
        assert_eq!(apply_indent("\tselect 1", IndentDirection::Out), "select 1");
    }

    #[test]
    fn toggle_line_comments_transforms_uncommented_to_commented() {
        // Direct unit test of the pure transform: given a SQL
        // string and a selection, we compute the new SQL by hand
        // and compare to what the algorithm should produce.
        let sql = "select 1\nfrom users";
        let new_sql = comment_segment(sql, 0..3);
        assert_eq!(new_sql, "-- select 1\n-- from users");
    }

    #[test]
    fn toggle_line_comments_transforms_commented_to_uncommented() {
        let sql = "-- select 1\n-- from users";
        let new_sql = uncomment_segment(sql);
        assert_eq!(new_sql, "select 1\nfrom users");
    }

    #[test]
    fn indent_in_keeps_blank_lines_blank() {
        let sql = "select 1\n\nfrom users";
        let new_sql = indent_segment(sql, IndentDirection::In);
        assert_eq!(new_sql, "  select 1\n\n  from users");
    }

    #[test]
    fn indent_out_keeps_no_indent_lines_untouched() {
        let sql = "select 1\nfrom users";
        let new_sql = indent_segment(sql, IndentDirection::Out);
        assert_eq!(new_sql, "select 1\nfrom users");
    }

    #[test]
    fn preview_statement_takes_first_non_empty_line() {
        assert_eq!(preview_statement("  \nSELECT 1\nFROM t"), "SELECT 1");
        assert_eq!(
            preview_statement("INSERT INTO t VALUES (1)"),
            "INSERT INTO t VALUES (1)"
        );
    }

    #[test]
    fn preview_statement_truncates_long_lines() {
        let long = "select ".to_string() + &"x".repeat(120);
        let preview = preview_statement(&long);
        assert!(preview.chars().count() <= 80);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn apply_optimized_sql_rejects_write_statements() {
        let result = apply_optimized_sql_impl("DELETE FROM orders");
        assert!(result.is_err());
    }

    #[test]
    fn apply_optimized_sql_accepts_read_only() {
        let result = apply_optimized_sql_impl("SELECT * FROM orders");
        assert!(result.is_ok());
    }
}
