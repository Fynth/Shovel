use crate::ExecutionPlan;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqlKeywordCase {
    Preserve,
    Uppercase,
    Lowercase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SqlFormatSettings {
    pub keyword_case: SqlKeywordCase,
    pub indent_width: u8,
    pub lines_between_queries: u8,
    pub inline: bool,
    pub joins_as_top_level: bool,
    pub max_inline_block: u8,
    pub max_inline_arguments: Option<u8>,
    pub max_inline_top_level: Option<u8>,
}

impl Default for SqlFormatSettings {
    fn default() -> Self {
        Self {
            keyword_case: SqlKeywordCase::Uppercase,
            indent_width: 2,
            lines_between_queries: 1,
            inline: false,
            joins_as_top_level: true,
            max_inline_block: 40,
            max_inline_arguments: Some(4),
            max_inline_top_level: Some(40),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TablePreviewSource {
    pub schema: Option<String>,
    pub table_name: String,
    pub qualified_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuerySort {
    pub column_name: String,
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryFilterMode {
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryFilterOperator {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryFilterRule {
    pub column_name: String,
    pub operator: QueryFilterOperator,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryFilter {
    pub mode: QueryFilterMode,
    pub rules: Vec<QueryFilterRule>,
}

impl QueryFilterOperator {
    pub fn is_nullary(self) -> bool {
        matches!(self, Self::IsNull | Self::IsNotNull)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditableTableContext {
    pub source: TablePreviewSource,
    pub row_locators: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PendingTableChanges {
    pub next_insert_id: u64,
    pub inserted_rows: Vec<PendingInsertRow>,
    pub updated_cells: Vec<PendingCellChange>,
    pub deleted_rows: Vec<PendingDeleteRow>,
}

impl PendingTableChanges {
    pub fn is_empty(&self) -> bool {
        self.inserted_rows.is_empty()
            && self.updated_cells.is_empty()
            && self.deleted_rows.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingInsertRow {
    pub id: u64,
    pub values: Vec<Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCellChange {
    pub locator: String,
    pub column_name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingDeleteRow {
    pub locator: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub editable: Option<EditableTableContext>,
    pub offset: u64,
    pub page_size: u32,
    pub has_previous: bool,
    pub has_next: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueryOutput {
    Table(QueryPage),
    AffectedRows(u64),
}

/// One statement's outcome inside a multi-statement batch run.
///
/// Stored in `QueryTabState::batch_results` so the UI can render a
/// tab strip of per-statement results + a final Status tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchResult {
    /// 0-based index in the original script (matches `Statement::index`).
    pub index: usize,
    /// First line (0-based) of the statement in the original SQL.
    pub line: usize,
    /// Short preview of the statement (first non-whitespace line, max ~80 chars).
    pub preview: String,
    pub outcome: BatchOutcome,
    /// `Some(duration_ms)` on success.
    pub duration_ms: Option<u64>,
    /// `Some(rows)` on success (Table → len, AffectedRows → count).
    pub rows: Option<usize>,
    /// `Some(message)` on error.
    pub error_message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchOutcome {
    /// Executed without error.
    Ok,
    /// Execution returned an error from the server.
    Error,
    /// Not executed because a previous statement in the batch failed.
    Skipped,
    /// Batch is still running this statement.
    Running,
}

impl BatchOutcome {
    pub fn is_ok(self) -> bool {
        matches!(self, BatchOutcome::Ok)
    }
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            BatchOutcome::Ok | BatchOutcome::Error | BatchOutcome::Skipped
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchTransactionState {
    /// No transaction was used (read-only batch, or ClickHouse).
    None,
    /// `BEGIN` was sent, batch is in progress.
    InProgress,
    /// `COMMIT` was sent at the end.
    Committed,
    /// `ROLLBACK` was sent after a failed statement.
    RolledBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceTabKind {
    Query,
    TablePreview,
    Structure,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryTabState {
    pub id: u64,
    pub session_id: u64,
    pub title: String,
    pub sql: String,
    pub status: String,
    pub result: Option<QueryOutput>,
    pub current_offset: u64,
    pub page_size: u32,
    pub last_run_sql: Option<String>,
    pub preview_source: Option<TablePreviewSource>,
    pub filter: Option<QueryFilter>,
    pub sort: Option<QuerySort>,
    pub tab_kind: WorkspaceTabKind,
    pub is_loading_more: bool,
    pub pending_table_changes: PendingTableChanges,
    pub execution_plan: Option<ExecutionPlan>,
    pub show_execution_plan: bool,
    /// Multi-statement batch state. `Some(...)` when a batch run is in
    /// progress or has just completed; `None` for single-statement runs.
    ///
    /// UI uses this to render a tab strip of per-statement results.
    pub batch_results: Option<BatchRunState>,
    /// Длительность последнего выполнения запроса (мс). None, если запрос
    /// ещё не выполнялся. Используется для индикации тайминга в области
    /// результатов и статусе вкладки.
    pub last_duration_ms: Option<u64>,
}

/// In-progress or completed multi-statement batch run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRunState {
    /// All non-empty statements in the batch, in order. Length is
    /// stable for the lifetime of the run; outcomes are mutated in place.
    pub results: Vec<BatchResult>,
    /// Index of the currently-rendering tab in the result strip.
    /// `0..results.len()` selects a per-statement tab; `results.len()`
    /// selects the summary "Status" tab.
    pub active_index: usize,
    /// Server-side transaction state (only meaningful for PG/MySQL/SQLite
    /// batches that include at least one write).
    pub tx_state: BatchTransactionState,
    /// Total wall-clock duration of the batch in milliseconds.
    pub total_duration_ms: u64,
}

impl Default for BatchRunState {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            active_index: 0,
            tx_state: BatchTransactionState::None,
            total_duration_ms: 0,
        }
    }
}

impl Default for QueryTabState {
    fn default() -> Self {
        Self {
            id: 0,
            session_id: 0,
            title: String::new(),
            sql: String::new(),
            status: String::new(),
            result: None,
            current_offset: 0,
            page_size: 100,
            last_run_sql: None,
            preview_source: None,
            filter: None,
            sort: None,
            tab_kind: WorkspaceTabKind::Query,
            is_loading_more: false,
            pending_table_changes: PendingTableChanges::default(),
            execution_plan: None,
            show_execution_plan: false,
            batch_results: None,
            last_duration_ms: None,
        }
    }
}

/// Metrics collected during query execution.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionMetrics {
    pub duration_ms: u64,
    pub rows_returned: Option<usize>,
    pub error_details: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryHistoryItem {
    pub id: u64,
    pub tab_title: String,
    #[serde(default)]
    pub connection_name: String,
    pub sql: String,
    pub outcome: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub rows_returned: Option<usize>,
    #[serde(default)]
    pub executed_at: i64,
    #[serde(default)]
    pub connection_type: String,
    #[serde(default)]
    pub error_message: Option<String>,
}

/// Filter for searching query history.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryHistoryFilter {
    pub from_date: Option<i64>,
    pub to_date: Option<i64>,
    pub connection: Option<String>,
    pub error_status: Option<QueryHistoryErrorStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryHistoryErrorStatus {
    Success,
    Failed,
    Any,
}

/// Persisted SQL draft for a query tab. Linked to a connection
/// via `session_identity_key` (not runtime `session_id`, which
/// changes between launches). Only `title` and `sql` are saved —
/// results, filters, and other runtime state are not persisted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabDraft {
    pub session_identity_key: String,
    pub title: String,
    pub sql: String,
}
