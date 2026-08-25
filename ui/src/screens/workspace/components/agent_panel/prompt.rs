use dioxus::prelude::*;
use models::{
    AcpMessageKind,
    AcpPanelState,
    AcpUiMessage,
    QueryOutput,
    QueryPage,
    QueryTabState,
    TablePreviewSource,
    WorkspaceTabKind,
};

use crate::{
    app_state::{session_connection, set_show_sql_editor},
    screens::workspace::actions::update_active_tab_sql,
};

use super::state::push_message;

const MAX_ACTIVE_RESULT_ROWS: usize = 5;

/// Thread-safe storage for the AI response language preference.
/// Updated from `APP_UI_SETTINGS` when settings change; defaults to English.
static AI_RESPONSE_LANGUAGE: std::sync::LazyLock<std::sync::RwLock<String>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new("English".to_string()));

/// Update the AI response language from UI settings. Called when
/// settings are loaded or changed.
pub(crate) fn sync_ai_response_language(language: String) {
    if let Ok(mut guard) = AI_RESPONSE_LANGUAGE.write() {
        *guard = language;
    }
}

/// Returns the "Always answer in X" directive based on the user's
/// AI response language setting. Defaults to English.
fn response_language_directive() -> String {
    let language = AI_RESPONSE_LANGUAGE
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "English".to_string());
    if language.trim().is_empty() {
        "Always answer in English.\n".to_string()
    } else {
        format!("Always answer in {language}.\n")
    }
}

pub(crate) fn extract_sql_candidate(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(sql) = extract_fenced_block(trimmed, "sql") {
        return Some(sql);
    }
    if let Some(sql) = extract_any_fenced_block(trimmed) {
        return Some(sql);
    }

    let lowered = trimmed.to_ascii_lowercase();
    [
        "select", "with", "insert", "update", "delete", "create", "alter", "drop", "truncate",
    ]
    .iter()
    .any(|keyword| lowered.starts_with(keyword))
    .then(|| trimmed.to_string())
}

fn extract_fenced_block(text: &str, language: &str) -> Option<String> {
    let needle = format!("```{language}");
    let start = text.find(&needle)?;
    let rest = &text[start + needle.len()..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

fn extract_any_fenced_block(text: &str) -> Option<String> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let rest = match rest.find('\n') {
        Some(newline) => &rest[newline + 1..],
        None => rest,
    };
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

pub(super) fn build_sql_generation_prompt(
    connection_label: &str,
    request: &str,
    db_context: Option<String>,
    active_tab_context: Option<String>,
    thread_history: Option<String>,
) -> String {
    let mut prompt = format!(
        "You are generating SQL for the active database connection.\n\
Database context: {connection_label}\n"
    );
    if let Some(thread_history) = thread_history {
        prompt.push_str("Use this recent chat history for follow-up intent:\n");
        prompt.push_str(&thread_history);
        prompt.push('\n');
    }
    if let Some(active_tab_context) = active_tab_context {
        prompt.push_str("Use this active editor context too:\n");
        prompt.push_str(&active_tab_context);
        prompt.push('\n');
    }
    if let Some(db_context) = db_context {
        prompt.push_str("Use this live database snapshot:\n");
        prompt.push_str(&db_context);
        prompt.push('\n');
    }
    prompt.push_str(
        "Snapshot rows are previews only. Never infer total row counts, aggregates, or full-table statistics unless a query result explicitly provides them.\n\
If the available context is insufficient, generate SQL that verifies the answer instead of guessing.\n\
Use the existing ACP session history for follow-up requests, but prefer the current editor and database context when they conflict with older assumptions.\n\
Never invent database, schema, view, or table names. Use only exact relation names that appear in the live database snapshot or active editor context.\n\
Never synthesize suffixes or prefixes such as `_kafka`, `_buffer`, `_mv`, `_view`, `_tmp`, or `_staging` unless that exact relation name is present in context.\n\
When the user asks for rows from a specific relation and the schema is available; expand the real column list from context.\n\
Prefer the fully qualified relation name when schema or database names are available.\n\
For ClickHouse, do not target Kafka, RabbitMQ, NATS, S3Queue, AzureQueue, or Redis ingest relations for an ordinary SELECT unless that exact relation is present in context and the user explicitly asks for a direct read.\n\
Do not add LIMIT, OFFSET, TOP, FETCH, SAMPLE, or TABLESAMPLE unless the user explicitly asks for it.\n\
When creating tables, always define an auto-generated primary key `id`.\n\
For SQLite use `id INTEGER PRIMARY KEY AUTOINCREMENT`.\n\
For PostgreSQL use `id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY`.\n\
When inserting rows, omit the `id` column unless the user explicitly asks to provide it manually.\n\
Return exactly one SQL query inside a single ```sql``` block with no explanation.\n",
    );
    prompt.push_str(&format!("User request: {request}"));
    prompt
}

pub(super) fn build_sql_explanation_prompt(
    connection_label: &str,
    active_sql: &str,
    db_context: Option<String>,
    active_tab_context: Option<String>,
    thread_history: Option<String>,
) -> String {
    let mut prompt = format!(
        "You are reviewing SQL for the active database connection.\n\
Database context: {connection_label}\n\
Active SQL:\n```sql\n{active_sql}\n```\n"
    );
    if let Some(thread_history) = thread_history {
        prompt.push_str("Use this recent chat history for follow-up intent:\n");
        prompt.push_str(&thread_history);
        prompt.push('\n');
    }
    if let Some(active_tab_context) = active_tab_context {
        prompt.push_str("Use this active editor context too:\n");
        prompt.push_str(&active_tab_context);
        prompt.push('\n');
    }
    if let Some(db_context) = db_context {
        prompt.push_str("Use this live database snapshot:\n");
        prompt.push_str(&db_context);
        prompt.push('\n');
    }
    prompt.push_str(
        &(response_language_directive() + "Explain what the SQL does, what assumptions it makes, and any correctness or performance risks.\n\
If the query looks wrong or unsafe, say so clearly.\n\
Do not add LIMIT, OFFSET, TOP, FETCH, SAMPLE, or TABLESAMPLE unless the user explicitly asks for it or the original SQL already uses one.\n\
If a better read-only alternative is appropriate, include exactly one improved SQL query inside a single ```sql``` block.\n"),
    );
    prompt
}

pub(super) fn build_sql_plan_prompt(
    connection_label: &str,
    active_sql: &str,
    explain_sql: &str,
    explain_plan: &str,
    db_context: Option<String>,
    active_tab_context: Option<String>,
    thread_history: Option<String>,
) -> String {
    let mut prompt = format!(
        "You are reviewing a database query plan for the active connection.\n\
Database context: {connection_label}\n\
Active SQL:\n```sql\n{active_sql}\n```\n\
Explain SQL:\n```sql\n{explain_sql}\n```\n\
Explain plan snapshot:\n```\n{explain_plan}\n```\n"
    );
    if let Some(thread_history) = thread_history {
        prompt.push_str("Use this recent chat history for follow-up intent:\n");
        prompt.push_str(&thread_history);
        prompt.push('\n');
    }
    if let Some(active_tab_context) = active_tab_context {
        prompt.push_str("Use this active editor context too:\n");
        prompt.push_str(&active_tab_context);
        prompt.push('\n');
    }
    if let Some(db_context) = db_context {
        prompt.push_str("Use this live database snapshot:\n");
        prompt.push_str(&db_context);
        prompt.push('\n');
    }
    prompt.push_str(
        &(response_language_directive()
            + "Analyze the query plan and return your findings as a single fenced ```json block with EXACTLY this shape (no other text outside the block):\n\
```json\n\
{\n\
  \"summary\": \"one-paragraph plain-language explanation\",\n\
  \"recommendations\": [\n\
    {\n\
      \"severity\": \"critical|warning|info\",\n\
      \"category\": \"scan|join|sort|index|other\",\n\
      \"title\": \"short title\",\n\
      \"detail\": \"explanation\",\n\
      \"suggested_index\": \"CREATE INDEX ... or null\"\n\
    }\n\
  ],\n\
  \"rewritten_sql\": \"improved read-only SQL or null\"\n\
}\n\
```\n\
Rules: severity and category must be lowercase. Do not invent exact costs or row counts beyond what the plan shows. Do not add LIMIT/OFFSET/TOP/FETCH/SAMPLE/TABLESAMPLE unless the original SQL already uses one. If a better read-only rewrite is obvious, put it in rewritten_sql; otherwise null.\n"),
    );
    prompt
}

pub(super) fn insert_sql_into_editor(
    mut panel_state: Signal<AcpPanelState>,
    tabs: Signal<Vec<QueryTabState>>,
    mut active_tab_id: Signal<u64>,
    sql: String,
) {
    let Some(target_tab_id) = preferred_sql_target_tab_id(tabs, active_tab_id()) else {
        panel_state.with_mut(|state| {
            state.status = "No active SQL tab to insert into.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "No active SQL tab to insert into.".to_string(),
            );
        });
        return;
    };

    set_show_sql_editor(true);
    active_tab_id.set(target_tab_id);
    update_active_tab_sql(
        tabs,
        target_tab_id,
        sql,
        "SQL inserted from ACP agent".to_string(),
    );
    panel_state.with_mut(|state| {
        state.busy = false;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Inserted agent SQL into the active editor.".to_string();
        state
            .messages
            .retain(|message| !matches!(message.kind, AcpMessageKind::Thought));
    });
}

pub(crate) fn preferred_sql_target_tab_id(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<u64> {
    preferred_sql_target_tab_id_from_tabs(&tabs.read(), active_tab_id)
}

fn preferred_sql_target_tab_id_from_tabs(
    tabs: &[QueryTabState],
    active_tab_id: u64,
) -> Option<u64> {
    let active_tab = tabs.iter().find(|tab| tab.id == active_tab_id);

    if let Some(tab) = active_tab.filter(|tab| matches!(tab.tab_kind, WorkspaceTabKind::Query)) {
        return Some(tab.id);
    }

    if let Some(session_id) = active_tab.map(|tab| tab.session_id)
        && let Some(query_tab) = tabs.iter().find(|tab| {
            tab.session_id == session_id && matches!(tab.tab_kind, WorkspaceTabKind::Query)
        })
    {
        return Some(query_tab.id);
    }

    tabs.iter()
        .find(|tab| matches!(tab.tab_kind, WorkspaceTabKind::Query))
        .map(|tab| tab.id)
        .or_else(|| tabs.first().map(|tab| tab.id))
}

pub(super) fn build_chat_prompt(
    connection_label: &str,
    prompt: &str,
    db_context: Option<String>,
    active_tab_context: Option<String>,
    thread_history: Option<String>,
) -> String {
    let mut message = format!(
        "You are helping with the active database connection.\n\
Database context: {connection_label}\n"
    );
    if let Some(thread_history) = thread_history {
        message.push_str("Use this recent chat history for follow-up context:\n");
        message.push_str(&thread_history);
        message.push('\n');
    }
    if let Some(active_tab_context) = active_tab_context {
        message.push_str("Use this active editor context when it is relevant:\n");
        message.push_str(&active_tab_context);
        message.push('\n');
    }
    if let Some(db_context) = db_context {
        message.push_str("Use this live database snapshot when answering:\n");
        message.push_str(&db_context);
        message.push('\n');
    }
    message.push_str(
        &(response_language_directive() + "Snapshot rows are previews only. Never infer total row counts, aggregates, or full-table statistics unless a query result explicitly provides them.\n\
If the available context is insufficient, say what is unknown and include exactly one read-only SQL query inside a single ```sql``` block so the app can verify it automatically.\n\
Use the ongoing ACP session history for follow-up questions, but do not invent facts that were not established earlier in the session.\n\
Prefer facts from the active editor context over generic assumptions.\n\
Do not add LIMIT, OFFSET, TOP, FETCH, SAMPLE, or TABLESAMPLE unless the user explicitly asks for it.\n\
If you propose schema creation, always use an auto-generated primary key `id`.\n\
If you propose inserts, omit `id` unless the user explicitly asks for manual ids.\n"),
    );
    message.push_str(&format!("User request: {prompt}"));
    message
}

pub(super) fn build_thread_history_context(messages: &[AcpUiMessage]) -> Option<String> {
    const MAX_THREAD_MESSAGES: usize = 12;

    let transcript = messages
        .iter()
        .filter(|message| {
            !matches!(message.kind, AcpMessageKind::Thought | AcpMessageKind::Tool)
                && !message.text.trim().is_empty()
                && !message.text.starts_with("Connected to ")
        })
        .rev()
        .take(MAX_THREAD_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            format!(
                "{}: {}",
                thread_role_label(&message.kind),
                message.text.trim()
            )
        })
        .collect::<Vec<_>>();

    if transcript.is_empty() {
        None
    } else {
        Some(transcript.join("\n"))
    }
}

pub(super) fn active_editor_prompt_context(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<String> {
    let tabs = tabs.read();
    let tab = tabs.iter().find(|tab| tab.id == active_tab_id)?;
    build_active_tab_context(tab)
}

pub(super) fn active_editor_sql(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<String> {
    tabs.read()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .map(|tab| tab.sql.trim().to_string())
        .filter(|sql| !sql.is_empty())
}

pub(super) fn active_editor_error(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<String> {
    let status = tabs
        .read()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .map(|tab| tab.status.trim().to_string())?;

    extract_status_error(&status)
}

fn extract_status_error(status: &str) -> Option<String> {
    [
        "Error: ",
        "Preview error: ",
        "Structure error: ",
        "Load more error: ",
    ]
    .iter()
    .find_map(|prefix| status.strip_prefix(prefix))
    .map(str::trim)
    .filter(|message| !message.is_empty())
    .map(ToOwned::to_owned)
}

fn build_active_tab_context(tab: &QueryTabState) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(source) = tab.preview_source.as_ref() {
        sections.push(format!("Focused relation: {}", source.qualified_name));
    }

    let sql = tab.sql.trim();
    if !sql.is_empty() {
        sections.push(format!("Active editor SQL:\n```sql\n{sql}\n```"));
    }

    if let Some(last_run_sql) = tab
        .last_run_sql
        .as_deref()
        .map(str::trim)
        .filter(|last_run_sql| !last_run_sql.is_empty() && *last_run_sql != sql)
    {
        sections.push(format!("Last executed SQL:\n```sql\n{last_run_sql}\n```"));
    }

    let status = tab.status.trim();
    if !status.is_empty() {
        sections.push(format!("Active tab status: {status}"));
    }

    if !tab.pending_table_changes.is_empty() {
        sections.push(format!(
            "Pending local table edits: {} inserted row(s), {} edited cell(s).",
            tab.pending_table_changes.inserted_rows.len(),
            tab.pending_table_changes.updated_cells.len()
        ));
    }

    if let Some(result) = &tab.result {
        sections.push(build_result_context(result));
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn build_result_context(result: &QueryOutput) -> String {
    describe_query_output("Active tab result", result)
}

pub(super) fn describe_query_output(label: &str, result: &QueryOutput) -> String {
    match result {
        QueryOutput::AffectedRows(rows) => {
            format!("{label}: the last statement affected {rows} row(s).")
        }
        QueryOutput::Table(page) => build_page_result_context(label, page),
    }
}

fn thread_role_label(kind: &AcpMessageKind) -> &'static str {
    match kind {
        AcpMessageKind::User => "User",
        AcpMessageKind::Agent => "Assistant",
        AcpMessageKind::Thought => "Thought",
        AcpMessageKind::Tool => "Tool",
        AcpMessageKind::System => "System",
        AcpMessageKind::Error => "Error",
    }
}

fn build_page_result_context(label: &str, page: &QueryPage) -> String {
    if page.columns.is_empty() && page.rows.is_empty() {
        return format!("{label}: the query returned no rows.");
    }

    let mut lines = Vec::new();
    if page.rows.is_empty() {
        lines.push(format!("{label} preview: no rows on the current page."));
    } else {
        let first_row = page.offset + 1;
        let last_row = page.offset + page.rows.len() as u64;
        let preview_scope = if page.has_next {
            " More rows exist beyond this preview."
        } else {
            ""
        };
        lines.push(format!(
            "{label} preview: rows {first_row}-{last_row} from the current page.{preview_scope}"
        ));
    }

    if !page.columns.is_empty() {
        lines.push(format!("Result columns: {}", page.columns.join(", ")));
    }

    for row in page.rows.iter().take(MAX_ACTIVE_RESULT_ROWS) {
        let cells = page
            .columns
            .iter()
            .zip(row.iter())
            .map(|(column, value)| format!("{column}={value}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("result row: {cells}"));
    }

    lines.join("\n")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        MAX_ACTIVE_RESULT_ROWS,
        build_active_tab_context,
        build_chat_prompt,
        build_sql_explanation_prompt,
        build_sql_generation_prompt,
        build_sql_plan_prompt,
        describe_query_output,
        extract_status_error,
        preferred_sql_target_tab_id_from_tabs,
    };
    use models::{QueryOutput, QueryPage, QueryTabState, WorkspaceTabKind};

    #[test]
    fn chat_prompt_requires_english_and_preview_safety() {
        let prompt = build_chat_prompt(
            "SQLite",
            "Summarize products",
            Some("preview".to_string()),
            Some("editor".to_string()),
            None,
        );
        assert!(prompt.contains("Always answer in English."));
        assert!(prompt.contains("Never infer total row counts"));
        assert!(prompt.contains("Use this active editor context"));
        assert!(prompt.contains("single ```sql``` block"));
        assert!(prompt.contains("verify it automatically"));
    }

    #[test]
    fn sql_prompt_warns_about_preview_only_rows() {
        let prompt = build_sql_generation_prompt(
            "SQLite",
            "Count products",
            Some("preview".to_string()),
            Some("editor".to_string()),
            None,
        );
        assert!(prompt.contains("Snapshot rows are previews only."));
        assert!(prompt.contains("Never infer total row counts"));
        assert!(prompt.contains("generate SQL that verifies"));
        assert!(prompt.contains("Do not add LIMIT, OFFSET, TOP, FETCH, SAMPLE, or TABLESAMPLE"));
        assert!(prompt.contains("Never invent database, schema, view, or table names."));
        assert!(prompt.contains("Never synthesize suffixes or prefixes"));
    }

    #[test]
    fn chat_prompt_forbids_implicit_row_limits() {
        let prompt = build_chat_prompt(
            "SQLite",
            "Show products",
            Some("preview".to_string()),
            Some("editor".to_string()),
            None,
        );
        assert!(prompt.contains("Do not add LIMIT, OFFSET, TOP, FETCH, SAMPLE, or TABLESAMPLE"));
    }

    #[test]
    fn sql_explanation_prompt_mentions_active_sql() {
        let prompt =
            build_sql_explanation_prompt("SQLite", "select * from products", None, None, None);
        assert!(prompt.contains("Active SQL:"));
        assert!(prompt.contains("Explain what the SQL does"));
    }

    #[test]
    fn sql_plan_prompt_mentions_explain_snapshot() {
        let prompt = build_sql_plan_prompt(
            "SQLite",
            "select * from products",
            "EXPLAIN select * from products",
            "Explain plan result preview: rows 1-1 from the current page.",
            None,
            None,
            None,
        );
        assert!(prompt.contains("Explain SQL:"));
        assert!(prompt.contains("Explain plan snapshot:"));
        assert!(prompt.contains("Analyze the query plan and return your findings"));
    }

    #[test]
    fn plan_prompt_requests_structured_json() {
        let prompt = build_sql_plan_prompt(
            "test-db",
            "SELECT * FROM orders",
            "EXPLAIN SELECT * FROM orders",
            "Seq Scan on orders",
            None,
            None,
            None,
        );
        assert!(prompt.contains("```json"));
        assert!(prompt.contains("rewritten_sql"));
        assert!(prompt.contains("suggested_index"));
    }

    #[test]
    fn describe_query_output_uses_custom_label() {
        let context = describe_query_output(
            "Explain plan result",
            &QueryOutput::Table(QueryPage {
                columns: vec!["plan".to_string()],
                rows: vec![vec!["SCAN products".to_string()]],
                editable: None,
                offset: 0,
                page_size: 100,
                has_previous: false,
                has_next: false,
            }),
        );
        assert!(context.contains("Explain plan result preview"));
        assert!(context.contains("result row: plan=SCAN products"));
    }

    #[test]
    fn active_tab_context_includes_sql_status_and_result_preview() {
        let tab = QueryTabState {
            id: 1,
            session_id: 1,
            title: "Query 1".to_string(),
            sql: "select * from products limit 100;".to_string(),
            status: "Loaded rows 1-10 from products".to_string(),
            result: Some(QueryOutput::Table(QueryPage {
                columns: vec!["id".to_string(), "name".to_string()],
                rows: (1..=MAX_ACTIVE_RESULT_ROWS as u64)
                    .map(|id| vec![id.to_string(), format!("Product {id}")])
                    .collect(),
                editable: None,
                offset: 0,
                page_size: 100,
                has_previous: false,
                has_next: true,
            })),
            ..QueryTabState::default()
        };

        let context = build_active_tab_context(&tab).expect("expected active tab context");
        assert!(context.contains("Active editor SQL"));
        assert!(context.contains("Loaded rows 1-10 from products"));
        assert!(context.contains("More rows exist beyond this preview."));
        assert!(context.contains("result row: id=1, name=Product 1"));
    }

    #[test]
    fn extracts_active_tab_error_from_status() {
        assert_eq!(
            extract_status_error("Error: SQLite error: no such table: missing"),
            Some("SQLite error: no such table: missing".to_string())
        );
    }

    #[test]
    fn prefers_query_tab_in_same_session_for_sql_inserts() {
        let tabs = vec![
            QueryTabState {
                id: 7,
                session_id: 3,
                title: "Preview".to_string(),
                tab_kind: WorkspaceTabKind::TablePreview,
                ..QueryTabState::default()
            },
            QueryTabState {
                id: 8,
                session_id: 3,
                title: "Query 8".to_string(),
                tab_kind: WorkspaceTabKind::Query,
                ..QueryTabState::default()
            },
        ];

        assert_eq!(preferred_sql_target_tab_id_from_tabs(&tabs, 7), Some(8));
    }
}

pub(super) fn active_editor_connection(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<models::DatabaseConnection> {
    let session_id = tabs
        .read()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .map(|tab| tab.session_id)?;
    session_connection(session_id)
}

pub(super) fn active_editor_focus_source(
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
) -> Option<TablePreviewSource> {
    tabs.read()
        .iter()
        .find(|tab| tab.id == active_tab_id)
        .and_then(|tab| tab.preview_source.clone())
}
