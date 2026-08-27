use std::path::PathBuf;

use database::{LiveConnection, SessionHandle};
use models::{
    DatabaseError,
    ExecutionPlan,
    ExplorerNode,
    ExplorerNodeKind,
    QueryFilter,
    QueryOutput,
    QuerySort,
    SqlFormatSettings,
    TableForeignKey,
    TablePreviewSource,
};

fn session_handle(session_id: u64) -> Result<SessionHandle, DatabaseError> {
    connection::session(session_id).ok_or(DatabaseError::SessionNotFound(session_id))
}

fn live_connection(session_id: u64) -> Result<LiveConnection, DatabaseError> {
    session_handle(session_id)?
        .legacy()
        .ok_or_else(|| DatabaseError::Unsupported("session has no live connection".into()))
}

pub fn format_sql_for_session(
    session_id: u64,
    sql: &str,
    settings: &SqlFormatSettings,
) -> Result<String, DatabaseError> {
    let handle = session_handle(session_id)?;
    Ok(query::format_sql(
        handle.dialect().format_flavor,
        sql,
        settings,
    ))
}

pub async fn execute_query(session_id: u64, sql: String) -> Result<QueryOutput, DatabaseError> {
    query::execute_query(&session_handle(session_id)?, sql).await
}

pub async fn execute_query_page(
    session_id: u64,
    sql: String,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    query::execute_query_page(
        &session_handle(session_id)?,
        sql,
        page_size,
        offset,
        filter,
        sort,
    )
    .await
}

pub async fn execute_explain(
    session_id: u64,
    sql: &str,
    analyze: bool,
) -> Result<ExecutionPlan, DatabaseError> {
    query::execute_explain(&session_handle(session_id)?, sql, analyze).await
}

pub async fn load_table_preview_page(
    session_id: u64,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    query::load_table_preview_page(
        &session_handle(session_id)?,
        source,
        page_size,
        offset,
        filter,
        sort,
    )
    .await
}

pub async fn create_table(
    session_id: u64,
    schema: Option<String>,
    table_name: String,
    columns_sql: String,
    clickhouse_engine: Option<String>,
) -> Result<(), DatabaseError> {
    query::create_table(
        &session_handle(session_id)?,
        schema,
        table_name,
        columns_sql,
        clickhouse_engine,
    )
    .await
}

pub async fn drop_table(session_id: u64, source: TablePreviewSource) -> Result<(), DatabaseError> {
    query::drop_table(&session_handle(session_id)?, source).await
}

pub async fn truncate_table(
    session_id: u64,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    query::truncate_table(&session_handle(session_id)?, source).await
}

pub async fn rename_table(
    session_id: u64,
    source: TablePreviewSource,
    new_table_name: String,
) -> Result<(), DatabaseError> {
    query::rename_table(&session_handle(session_id)?, source, new_table_name).await
}

pub async fn duplicate_table(
    session_id: u64,
    source: TablePreviewSource,
    new_table_name: String,
    copy_data: bool,
) -> Result<(), DatabaseError> {
    query::duplicate_table(
        &session_handle(session_id)?,
        source,
        new_table_name,
        copy_data,
    )
    .await
}

pub async fn delete_table_row(
    session_id: u64,
    source: TablePreviewSource,
    locator: String,
) -> Result<(), DatabaseError> {
    query::delete_table_row(&session_handle(session_id)?, source, locator).await
}

pub async fn insert_table_row(
    session_id: u64,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    query::insert_table_row(&session_handle(session_id)?, source).await
}

pub async fn insert_table_row_with_values(
    session_id: u64,
    source: TablePreviewSource,
    column_values: Vec<(String, String)>,
) -> Result<(), DatabaseError> {
    query::insert_table_row_with_values(&session_handle(session_id)?, source, column_values).await
}

pub async fn next_table_primary_key_id(
    session_id: u64,
    source: TablePreviewSource,
) -> Result<Option<(String, i64)>, DatabaseError> {
    query::next_table_primary_key_id(&session_handle(session_id)?, source).await
}

pub async fn update_table_cell(
    session_id: u64,
    source: TablePreviewSource,
    locator: String,
    column_name: String,
    value: String,
) -> Result<(), DatabaseError> {
    query::update_table_cell(
        &session_handle(session_id)?,
        source,
        locator,
        column_name,
        value,
    )
    .await
}

pub async fn import_csv_into_table(
    session_id: u64,
    source: TablePreviewSource,
    path: PathBuf,
) -> Result<u64, String> {
    query::import_csv_into_table(
        &session_handle(session_id).map_err(|err| err.to_string())?,
        source,
        path,
    )
    .await
}

pub async fn describe_table(
    session_id: u64,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    explorer::describe_table(live_connection(session_id)?, schema, table).await
}

pub async fn load_table_columns(
    session_id: u64,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    explorer::load_table_columns(live_connection(session_id)?, schema, table).await
}

pub async fn load_connection_tree(session_id: u64) -> Result<Vec<ExplorerNode>, DatabaseError> {
    explorer::load_connection_tree(live_connection(session_id)?).await
}

pub async fn load_foreign_keys(session_id: u64) -> Result<Vec<TableForeignKey>, DatabaseError> {
    explorer::load_foreign_keys(live_connection(session_id)?).await
}

pub async fn load_object_ddl(
    session_id: u64,
    schema: Option<String>,
    object: String,
    kind: ExplorerNodeKind,
) -> Result<Option<String>, DatabaseError> {
    explorer::load_object_ddl(live_connection(session_id)?, schema, object, kind).await
}

pub async fn build_acp_database_context(
    session_id: u64,
    connection_label: String,
    focus_source: Option<TablePreviewSource>,
) -> Result<String, DatabaseError> {
    acp::build_acp_database_context(live_connection(session_id)?, connection_label, focus_source)
        .await
}

pub async fn warm_acp_database_schema_context(
    session_id: u64,
    connection_label: String,
) -> Result<(), DatabaseError> {
    acp::warm_acp_database_schema_context(live_connection(session_id)?, connection_label).await
}
