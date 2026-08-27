use database::SessionHandle;
use models::{DatabaseError, DatabaseKind, QueryOutput, TablePreviewSource};

use super::{
    qualified_mysql_table_name,
    qualified_postgres_table_name,
    qualified_sqlite_table_name,
    quote_identifier,
    quote_identifier_clickhouse,
    rewrite_create_table_statement,
    sql_literal,
};

fn qualified_clickhouse_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier_clickhouse(schema),
            quote_identifier_clickhouse(table_name)
        ),
        None => quote_identifier_clickhouse(table_name),
    }
}

async fn exec_sql(handle: &SessionHandle, sql: &str) -> Result<(), DatabaseError> {
    handle.query().execute_sql(sql).await.map(|_| ())
}

async fn first_cell(handle: &SessionHandle, sql: &str) -> Result<String, DatabaseError> {
    match handle.query().execute_sql(sql).await? {
        QueryOutput::Table(page) => page
            .rows
            .into_iter()
            .next()
            .and_then(|row| row.into_iter().next())
            .filter(|value| !value.trim().is_empty() && value != "NULL")
            .ok_or_else(|| {
                DatabaseError::Unsupported("Could not load CREATE TABLE statement".to_string())
            }),
        _ => Err(DatabaseError::Unsupported(
            "Could not load CREATE TABLE statement".to_string(),
        )),
    }
}

pub async fn create_table(
    handle: &SessionHandle,
    schema: Option<String>,
    table_name: String,
    columns_sql: String,
    clickhouse_engine: Option<String>,
) -> Result<(), DatabaseError> {
    let table_name = table_name.trim();
    let columns_sql = columns_sql.trim().trim_end_matches(';').trim();
    if table_name.is_empty() {
        return Err(DatabaseError::Unsupported(
            "Table name is empty".to_string(),
        ));
    }
    if columns_sql.is_empty() {
        return Err(DatabaseError::Unsupported(
            "Table definition is empty".to_string(),
        ));
    }

    let columns_sql = if columns_sql.starts_with('(') {
        columns_sql.to_string()
    } else {
        format!("(\n{columns_sql}\n)")
    };

    let sql = match handle.kind() {
        DatabaseKind::Sqlite => {
            let qualified_name = qualified_sqlite_table_name(schema.as_deref(), table_name);
            format!("create table {qualified_name} {columns_sql}")
        }
        DatabaseKind::Postgres => {
            let qualified_name = qualified_postgres_table_name(schema.as_deref(), table_name);
            format!("create table {qualified_name} {columns_sql}")
        }
        DatabaseKind::MySql => {
            let qualified_name = qualified_mysql_table_name(schema.as_deref(), table_name);
            format!("create table {qualified_name} {columns_sql}")
        }
        DatabaseKind::ClickHouse => {
            let engine = clickhouse_engine
                .map(|engine| engine.trim().trim_end_matches(';').trim().to_string())
                .filter(|engine| !engine.is_empty())
                .ok_or_else(|| {
                    DatabaseError::Unsupported("ClickHouse engine clause is empty".to_string())
                })?;
            let qualified_name = qualified_clickhouse_name(schema.as_deref(), table_name);
            format!("create table {qualified_name} {columns_sql} {engine}")
        }
    };
    exec_sql(handle, &sql).await
}

pub async fn drop_table(
    handle: &SessionHandle,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    let sql = format!(
        "drop table if exists {}",
        source.qualified_name.trim().trim_end_matches(';')
    );
    exec_sql(handle, &sql).await
}

pub async fn truncate_table(
    handle: &SessionHandle,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    let qualified_name = source.qualified_name.trim().trim_end_matches(';');
    let sql = if handle.kind() == DatabaseKind::Sqlite {
        format!("delete from {qualified_name}")
    } else {
        format!("truncate table {qualified_name}")
    };
    exec_sql(handle, &sql).await
}

pub async fn rename_table(
    handle: &SessionHandle,
    source: TablePreviewSource,
    new_table_name: String,
) -> Result<(), DatabaseError> {
    let new_table_name = new_table_name.trim();
    if new_table_name.is_empty() {
        return Err(DatabaseError::Unsupported(
            "New table name is empty".to_string(),
        ));
    }
    if new_table_name == source.table_name.trim() {
        return Err(DatabaseError::Unsupported(
            "New table name must be different from the source table".to_string(),
        ));
    }

    let source_qualified_name = source.qualified_name.trim().trim_end_matches(';');
    let sql = match handle.kind() {
        DatabaseKind::Sqlite | DatabaseKind::Postgres => format!(
            "alter table {source_qualified_name} rename to {}",
            quote_identifier(new_table_name)
        ),
        DatabaseKind::MySql => {
            let target_qualified_name =
                qualified_mysql_table_name(source.schema.as_deref(), new_table_name);
            format!("rename table {source_qualified_name} to {target_qualified_name}")
        }
        DatabaseKind::ClickHouse => {
            let target_qualified_name =
                qualified_clickhouse_name(source.schema.as_deref(), new_table_name);
            format!("rename table {source_qualified_name} to {target_qualified_name}")
        }
    };
    exec_sql(handle, &sql).await
}

pub async fn duplicate_table(
    handle: &SessionHandle,
    source: TablePreviewSource,
    new_table_name: String,
    copy_data: bool,
) -> Result<(), DatabaseError> {
    let new_table_name = new_table_name.trim();
    if new_table_name.is_empty() {
        return Err(DatabaseError::Unsupported(
            "New table name is empty".to_string(),
        ));
    }
    if new_table_name == source.table_name.trim() {
        return Err(DatabaseError::Unsupported(
            "New table name must be different from the source table".to_string(),
        ));
    }

    let source_qualified_name = source.qualified_name.trim().trim_end_matches(';');

    match handle.kind() {
        DatabaseKind::Sqlite => {
            let schema_name = source
                .schema
                .as_deref()
                .map(str::trim)
                .filter(|schema| !schema.is_empty())
                .unwrap_or("main");
            let target_qualified_name =
                qualified_sqlite_table_name(source.schema.as_deref(), new_table_name);
            let create_sql = format!(
                "select sql from {}.sqlite_master where type = 'table' and name = {}",
                quote_identifier(schema_name),
                sql_literal(&source.table_name)
            );
            let create_statement = first_cell(handle, &create_sql).await.map_err(|_| {
                DatabaseError::Unsupported(format!(
                    "Could not load CREATE TABLE statement for {}",
                    source.table_name
                ))
            })?;
            let create_sql =
                rewrite_create_table_statement(&create_statement, &target_qualified_name)?;
            exec_sql(handle, &create_sql).await?;
            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                exec_sql(handle, &insert_sql).await?;
            }
            Ok(())
        }
        DatabaseKind::Postgres => {
            let target_qualified_name =
                qualified_postgres_table_name(source.schema.as_deref(), new_table_name);
            let create_sql = format!(
                "create table {target_qualified_name} (like {source_qualified_name} including all)"
            );
            exec_sql(handle, &create_sql).await?;
            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                exec_sql(handle, &insert_sql).await?;
            }
            Ok(())
        }
        DatabaseKind::MySql => {
            let target_qualified_name =
                qualified_mysql_table_name(source.schema.as_deref(), new_table_name);
            let create_sql =
                format!("create table {target_qualified_name} like {source_qualified_name}");
            exec_sql(handle, &create_sql).await?;
            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                exec_sql(handle, &insert_sql).await?;
            }
            Ok(())
        }
        DatabaseKind::ClickHouse => {
            let target_qualified_name =
                qualified_clickhouse_name(source.schema.as_deref(), new_table_name);
            let schema_name = source
                .schema
                .as_deref()
                .map(str::trim)
                .filter(|schema| !schema.is_empty())
                .unwrap_or("");
            let show_sql = if schema_name.is_empty() {
                format!(
                    "SHOW CREATE TABLE {}",
                    quote_identifier_clickhouse(&source.table_name)
                )
            } else {
                format!(
                    "SHOW CREATE TABLE {}.{}",
                    quote_identifier_clickhouse(schema_name),
                    quote_identifier_clickhouse(&source.table_name),
                )
            };
            let create_statement = first_cell(handle, &show_sql).await?;
            let create_sql =
                rewrite_create_table_statement(&create_statement, &target_qualified_name)?;
            exec_sql(handle, &create_sql).await?;
            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                exec_sql(handle, &insert_sql).await?;
            }
            Ok(())
        }
    }
}
