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
    quote_ident_backtick,
};
use models::{
    Capabilities,
    ClickHouseFormData,
    DatabaseError,
    DatabaseKind,
    QueryFilterOperator,
    QueryOutput,
};

use crate::rows::{clickhouse_rows_to_page, clickhouse_rows_to_paginated_page};

pub struct ClickHouseSession {
    pub config: ClickHouseFormData,
}

impl ClickHouseSession {
    fn dialect() -> Dialect {
        Dialect {
            quote_identifier: quote_ident_backtick,
            filter_expression: clickhouse_filter_expression,
            format_flavor: FormatFlavor::Generic,
        }
    }
}

#[async_trait]
impl QueryExec for ClickHouseSession {
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError> {
        if statement_returns_rows(sql) {
            let response = crate::execute_json_query(&self.config, sql)
                .await
                .map_err(DatabaseError::Driver)?;
            return Ok(QueryOutput::Table(decode_clickhouse_rows(response, sql)));
        }

        crate::execute_text_query(&self.config, sql)
            .await
            .map_err(DatabaseError::Driver)?;
        Ok(QueryOutput::AffectedRows(0))
    }
}

impl DriverSession for ClickHouseSession {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::ClickHouse
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(DatabaseKind::ClickHouse)
    }

    fn dialect(&self) -> Dialect {
        Self::dialect()
    }

    fn as_mutate(&self) -> Option<&dyn MutateExec> {
        None
    }

    fn as_explain(&self) -> Option<&dyn ExplainExec> {
        Some(self)
    }

    fn as_introspect(&self) -> Option<&dyn IntrospectExec> {
        Some(self)
    }

    fn as_legacy(&self) -> Option<LiveConnection> {
        Some(LiveConnection::ClickHouse(self.config.clone()))
    }
}

fn decode_clickhouse_rows(
    response: models::ClickHouseJsonResponse,
    sql: &str,
) -> models::QueryPage {
    if let Some((limit, offset)) = trailing_limit_offset(sql) {
        clickhouse_rows_to_paginated_page(response, limit.saturating_sub(1) as u32, offset)
    } else {
        clickhouse_rows_to_page(response)
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

fn clickhouse_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let column = quote_ident_backtick(column_name);
    let text_expr = format!("lowerUTF8(toString({column}))");
    let lower_literal = format!("lowerUTF8({})", sql_literal(value));
    match operator {
        QueryFilterOperator::Contains => format!(
            "positionCaseInsensitiveUTF8(toString({column}), {}) > 0",
            sql_literal(value)
        ),
        QueryFilterOperator::NotContains => format!(
            "positionCaseInsensitiveUTF8(toString({column}), {}) = 0",
            sql_literal(value)
        ),
        QueryFilterOperator::Equals => format!("{text_expr} = {lower_literal}"),
        QueryFilterOperator::NotEquals => format!("{text_expr} != {lower_literal}"),
        QueryFilterOperator::StartsWith => {
            format!("startsWith({text_expr}, {lower_literal})")
        }
        QueryFilterOperator::EndsWith => {
            format!("endsWith({text_expr}, {lower_literal})")
        }
        QueryFilterOperator::IsNull => format!("isNull({column})"),
        QueryFilterOperator::IsNotNull => format!("isNotNull({column})"),
    }
}

fn sql_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("null") {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}
