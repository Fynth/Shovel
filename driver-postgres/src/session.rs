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
    quote_ident_double,
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
    postgres_preview_rows_to_paginated_page,
    postgres_rows_to_page,
    postgres_rows_to_paginated_page,
};

pub struct PostgresSession {
    pub pool: sqlx::PgPool,
}

impl PostgresSession {
    fn dialect() -> Dialect {
        Dialect {
            quote_identifier: quote_ident_double,
            filter_expression: postgres_filter_expression,
            format_flavor: FormatFlavor::Postgres,
        }
    }
}

#[async_trait]
impl QueryExec for PostgresSession {
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError> {
        if statement_returns_rows(sql) {
            let rows = sqlx::query(sql)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
            return Ok(QueryOutput::Table(decode_postgres_rows(rows, sql)));
        }

        let result = sqlx::query(sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(QueryOutput::AffectedRows(result.rows_affected()))
    }
}

#[async_trait]
impl SchemaExec for PostgresSession {
    async fn describe_table(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "postgres schema exec is not implemented yet".into(),
        ))
    }

    async fn load_table_columns(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "postgres schema exec is not implemented yet".into(),
        ))
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "postgres schema exec is not implemented yet".into(),
        ))
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "postgres schema exec is not implemented yet".into(),
        ))
    }

    async fn load_object_ddl(
        &self,
        _schema: Option<String>,
        _object: String,
        _kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        Err(DatabaseError::Unsupported(
            "postgres schema exec is not implemented yet".into(),
        ))
    }
}

impl DriverSession for PostgresSession {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(DatabaseKind::Postgres)
    }

    fn dialect(&self) -> Dialect {
        Self::dialect()
    }

    fn as_mutate(&self) -> Option<&dyn MutateExec> {
        None
    }

    fn as_explain(&self) -> Option<&dyn ExplainExec> {
        None
    }

    fn as_introspect(&self) -> Option<&dyn IntrospectExec> {
        None
    }

    fn as_legacy(&self) -> Option<LiveConnection> {
        Some(LiveConnection::Postgres(self.pool.clone()))
    }
}

fn decode_postgres_rows(rows: Vec<sqlx::postgres::PgRow>, sql: &str) -> models::QueryPage {
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
        postgres_preview_rows_to_paginated_page(rows, unknown_preview_source(), page_size, offset)
    } else if let Some((limit, offset)) = page_meta {
        postgres_rows_to_paginated_page(rows, limit.saturating_sub(1) as u32, offset)
    } else {
        postgres_rows_to_page(rows)
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

fn postgres_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let text_expr = format!("cast({} as text)", quote_ident_double(column_name));
    match operator {
        QueryFilterOperator::Contains => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_contains_literal(value)
            )
        }
        QueryFilterOperator::NotContains => {
            format!(
                "{text_expr} not ilike {} escape '\\'",
                sql_contains_literal(value)
            )
        }
        QueryFilterOperator::Equals => {
            format!("lower({text_expr}) = lower({})", sql_literal(value))
        }
        QueryFilterOperator::NotEquals => {
            format!("lower({text_expr}) != lower({})", sql_literal(value))
        }
        QueryFilterOperator::StartsWith => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_prefix_literal(value)
            )
        }
        QueryFilterOperator::EndsWith => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_suffix_literal(value)
            )
        }
        QueryFilterOperator::IsNull => format!("{} is null", quote_ident_double(column_name)),
        QueryFilterOperator::IsNotNull => {
            format!("{} is not null", quote_ident_double(column_name))
        }
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
