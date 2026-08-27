// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{
    Dialect,
    DriverSession,
    ExplainExec,
    FormatFlavor,
    IntrospectExec,
    LiveConnection,
    MutateExec,
    QueryExec,
    SchemaExec,
    quote_ident_backtick,
};
use models::{
    Capabilities,
    DatabaseError,
    DatabaseKind,
    ExplorerNode,
    ExplorerNodeKind,
    QueryFilterOperator,
    QueryOutput,
    TableForeignKey,
    TablePreviewSource,
};
use sqlx::{Column, Row};

use crate::rows::{
    LOCATOR_COLUMN,
    mysql_preview_rows_to_paginated_page,
    mysql_rows_to_page,
    mysql_rows_to_paginated_page,
};

pub struct MysqlSession {
    pub pool: sqlx::MySqlPool,
}

impl MysqlSession {
    fn dialect() -> Dialect {
        Dialect {
            quote_identifier: quote_ident_backtick,
            filter_expression: mysql_filter_expression,
            format_flavor: FormatFlavor::Generic,
        }
    }
}

#[async_trait]
impl QueryExec for MysqlSession {
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError> {
        if statement_returns_rows(sql) {
            let rows = sqlx::query(sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
            return Ok(QueryOutput::Table(decode_mysql_rows(rows, sql)));
        }

        let result = sqlx::query(sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(QueryOutput::AffectedRows(result.rows_affected()))
    }

    async fn locator_expression(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<Option<String>, DatabaseError> {
        let schema_name = mysql_effective_schema_name(&self.pool, schema.as_deref()).await?;
        let primary_key_columns =
            mysql_primary_key_columns(&self.pool, &schema_name, &table).await?;
        if primary_key_columns.is_empty() {
            Ok(None)
        } else {
            Ok(Some(mysql_locator_expression(&primary_key_columns)))
        }
    }
}

#[async_trait]
impl SchemaExec for MysqlSession {
    async fn describe_table(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "mysql schema exec is not implemented yet".into(),
        ))
    }

    async fn load_table_columns(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "mysql schema exec is not implemented yet".into(),
        ))
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "mysql schema exec is not implemented yet".into(),
        ))
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "mysql schema exec is not implemented yet".into(),
        ))
    }

    async fn load_object_ddl(
        &self,
        _schema: Option<String>,
        _object: String,
        _kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "mysql schema exec is not implemented yet".into(),
        ))
    }
}

impl DriverSession for MysqlSession {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::MySql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(DatabaseKind::MySql)
    }

    fn dialect(&self) -> Dialect {
        Self::dialect()
    }

    fn as_mutate(&self) -> Option<&dyn MutateExec> {
        Some(self)
    }

    fn as_explain(&self) -> Option<&dyn ExplainExec> {
        Some(self)
    }

    fn as_introspect(&self) -> Option<&dyn IntrospectExec> {
        None
    }

    fn as_legacy(&self) -> Option<LiveConnection> {
        Some(LiveConnection::MySql(self.pool.clone()))
    }
}

fn decode_mysql_rows(rows: Vec<sqlx::mysql::MySqlRow>, sql: &str) -> models::QueryPage {
    let has_locator = rows
        .first()
        .and_then(|row| row.columns().first())
        .is_some_and(|column| column.name() == LOCATOR_COLUMN);
    let page_meta = trailing_limit_offset(sql);

    if has_locator {
        let (page_size, offset) = match page_meta {
            Some((limit, offset)) => (limit.saturating_sub(1) as u32, offset),
            None => (rows.len() as u32, 0),
        };
        mysql_preview_rows_to_paginated_page(rows, unknown_preview_source(), page_size, offset)
    } else if let Some((limit, offset)) = page_meta {
        mysql_rows_to_paginated_page(rows, limit.saturating_sub(1) as u32, offset)
    } else {
        mysql_rows_to_page(rows)
    }
}

fn unknown_preview_source() -> TablePreviewSource {
    TablePreviewSource {
        schema: None,
        table_name: String::new(),
        qualified_name: String::new(),
    }
}

fn trailing_limit_offset(sql: &str) -> Option<(u64, u64)> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let lower = sql.to_ascii_lowercase();
    let limit_idx = lower.rfind(" limit ")?;
    let after = sql[limit_idx + " limit ".len()..].trim();
    let mut parts = after.split_whitespace();
    let limit = parts.next()?.parse::<u64>().ok()?;
    if !parts.next()?.eq_ignore_ascii_case("offset") {
        return None;
    }
    let offset = parts.next()?.parse::<u64>().ok()?;
    parts.next().is_none().then_some((limit, offset))
}

fn statement_returns_rows(sql: &str) -> bool {
    matches!(
        leading_keyword(sql).as_deref(),
        Some("select" | "with" | "show" | "describe" | "explain" | "pragma")
    )
}

fn leading_keyword(sql: &str) -> Option<String> {
    let bytes = sql.as_bytes();
    let mut index = 0;

    loop {
        while index < bytes.len()
            && (bytes[index].is_ascii_whitespace() || matches!(bytes[index], b'(' | b';'))
        {
            index += 1;
        }

        if index + 1 < bytes.len() && bytes[index] == b'-' && bytes[index + 1] == b'-' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }

        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }

        break;
    }

    let start = index;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
        index += 1;
    }

    (index > start).then(|| sql[start..index].to_ascii_lowercase())
}

fn mysql_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let column = quote_ident_backtick(column_name);
    let text_expr = format!("lower(cast({column} as char))");
    let lower_literal = format!("lower({})", sql_literal(value));
    match operator {
        QueryFilterOperator::Contains => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::NotContains => format!(
            "{text_expr} not like lower({}) escape '\\\\'",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::Equals => format!("{text_expr} = {lower_literal}"),
        QueryFilterOperator::NotEquals => format!("{text_expr} != {lower_literal}"),
        QueryFilterOperator::StartsWith => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_prefix_literal(value)
        ),
        QueryFilterOperator::EndsWith => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_suffix_literal(value)
        ),
        QueryFilterOperator::IsNull => format!("{column} is null"),
        QueryFilterOperator::IsNotNull => format!("{column} is not null"),
    }
}

fn sql_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("null") {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn sql_contains_literal(value: &str) -> String {
    let escaped = escape_like_literal(value);
    format!("'%{escaped}%'")
}

fn sql_prefix_literal(value: &str) -> String {
    let escaped = escape_like_literal(value);
    format!("'{escaped}%'")
}

fn sql_suffix_literal(value: &str) -> String {
    let escaped = escape_like_literal(value);
    format!("'%{escaped}'")
}

fn escape_like_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''")
}

pub(crate) fn mysql_locator_expression(pk_columns: &[String]) -> String {
    let args = pk_columns
        .iter()
        .map(|column| format!("cast({} as char)", quote_ident_backtick(column)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("json_array({args})")
}

pub(crate) fn parse_mysql_locator(
    locator: &str,
    pk_columns: &[String],
) -> Result<Vec<String>, DatabaseError> {
    let values = serde_json::from_str::<Vec<String>>(locator)
        .map_err(|_| DatabaseError::Unsupported("Invalid MySQL row locator".to_string()))?;

    if values.len() != pk_columns.len() {
        return Err(DatabaseError::Unsupported(
            "Invalid MySQL row locator".to_string(),
        ));
    }

    Ok(pk_columns
        .iter()
        .zip(values)
        .map(|(column, value)| {
            format!(
                "cast({} as char) = {}",
                quote_ident_backtick(column),
                sql_literal(&value)
            )
        })
        .collect())
}

pub(crate) async fn mysql_effective_schema_name(
    pool: &sqlx::MySqlPool,
    schema: Option<&str>,
) -> Result<String, DatabaseError> {
    if let Some(schema) = schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        return Ok(schema.to_string());
    }

    sqlx::query_scalar::<_, Option<String>>("select database()")
        .fetch_one(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            DatabaseError::Unsupported(
                "No MySQL database selected. Set a default database or use a qualified table name."
                    .to_string(),
            )
        })
}

pub(crate) async fn mysql_primary_key_columns(
    pool: &sqlx::MySqlPool,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<String>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select kcu.column_name
        from information_schema.table_constraints tc
        join information_schema.key_column_usage kcu
          on tc.constraint_name = kcu.constraint_name
         and tc.table_schema = kcu.table_schema
         and tc.table_name = kcu.table_name
        where tc.constraint_type = 'PRIMARY KEY'
          and tc.table_schema = ?
          and tc.table_name = ?
        order by kcu.ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("column_name")
                .map_err(|e| DatabaseError::Driver(e.to_string()))
        })
        .collect()
}

pub(crate) async fn mysql_single_primary_key_column(
    pool: &sqlx::MySqlPool,
    schema_name: &str,
    table_name: &str,
) -> Result<Option<(String, String)>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select
          kcu.column_name,
          cols.data_type
        from information_schema.table_constraints tc
        join information_schema.key_column_usage kcu
          on tc.constraint_name = kcu.constraint_name
         and tc.table_schema = kcu.table_schema
         and tc.table_name = kcu.table_name
        join information_schema.columns cols
          on cols.table_schema = kcu.table_schema
         and cols.table_name = kcu.table_name
         and cols.column_name = kcu.column_name
        where tc.constraint_type = 'PRIMARY KEY'
          and tc.table_schema = ?
          and tc.table_name = ?
        order by kcu.ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    if rows.len() != 1 {
        return Ok(None);
    }

    let row = &rows[0];
    let column_name = row
        .try_get::<String, _>("column_name")
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    let data_type = row
        .try_get::<String, _>("data_type")
        .unwrap_or_else(|_| String::new());
    Ok(Some((column_name, data_type)))
}
