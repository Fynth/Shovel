mod build;
mod ddl;
mod editable;
mod execution_plan;
mod mutations;
mod preview;
mod rows;

use database::DatabaseDriver;
use driver_clickhouse::ClickHouseDriver;
use models::{
    DatabaseConnection, DatabaseError, QueryFilter, QueryOutput, QuerySort, TablePreviewSource,
};
use sqlx::Row;

pub use ddl::{create_table, drop_table, duplicate_table, truncate_table};
pub use execution_plan::execute_explain;
pub use mutations::{
    delete_table_row, insert_table_row, insert_table_row_with_values, next_table_primary_key_id,
    update_table_cell,
};
pub use preview::load_table_preview_page;

use self::{
    build::{
        SqlBuildDialect, build_editable_paginated_query, build_outer_paginated_query,
        build_paginated_query, clickhouse_filter_expression, mysql_filter_expression,
        postgres_filter_expression, quote_identifier, quote_identifier_clickhouse, sql_literal,
        sqlite_filter_expression,
    },
    editable::editable_select_plan,
    rows::{
        clickhouse_rows_to_page, clickhouse_rows_to_paginated_page, invalid_sqlite_locator,
        mysql_preview_rows_to_paginated_page, mysql_rows_to_page, mysql_rows_to_paginated_page,
        postgres_preview_rows_to_paginated_page, postgres_rows_to_paginated_page,
        sqlite_preview_rows_to_paginated_page, sqlite_rows_to_paginated_page,
    },
};

const LOCATOR_COLUMN: &str = "__shovel_locator";
const SQLITE_DIALECT: SqlBuildDialect = SqlBuildDialect {
    quote_identifier,
    filter_expression: sqlite_filter_expression,
};
const POSTGRES_DIALECT: SqlBuildDialect = SqlBuildDialect {
    quote_identifier,
    filter_expression: postgres_filter_expression,
};
const MYSQL_DIALECT: SqlBuildDialect = SqlBuildDialect {
    quote_identifier: quote_identifier_clickhouse,
    filter_expression: mysql_filter_expression,
};
const CLICKHOUSE_DIALECT: SqlBuildDialect = SqlBuildDialect {
    quote_identifier: quote_identifier_clickhouse,
    filter_expression: clickhouse_filter_expression,
};

pub(crate) use self::rows::{postgres_rows_to_page, sqlite_rows_to_page};

pub async fn execute_query(
    connection: DatabaseConnection,
    sql: String,
) -> Result<QueryOutput, DatabaseError> {
    execute_query_page(connection, sql, 100, 0, None, None).await
}

pub fn is_read_only_sql(sql: &str) -> bool {
    let keywords = statement_leading_keywords(sql);
    !keywords.is_empty()
        && keywords.iter().all(|keyword| {
            matches!(
                keyword.as_str(),
                "select" | "with" | "show" | "describe" | "explain" | "pragma"
            )
        })
}

pub fn preview_source_for_sql(sql: &str) -> Option<TablePreviewSource> {
    editable_select_plan(sql).map(|plan| plan.source)
}

pub async fn execute_query_page(
    connection: DatabaseConnection,
    sql: String,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            execute_sqlite_query_page(&sql, &pool, page_size, offset, filter, sort).await
        }
        DatabaseConnection::Postgres(pool) => {
            execute_postgres_query_page(&sql, &pool, page_size, offset, filter, sort).await
        }
        DatabaseConnection::MySql(pool) => {
            execute_mysql_query_page(&sql, &pool, page_size, offset, filter, sort).await
        }
        DatabaseConnection::ClickHouse(config) => {
            execute_clickhouse_query_page(&sql, &config, page_size, offset, filter, sort).await
        }
    }
}

async fn execute_sqlite_query_page(
    sql: &str,
    pool: &sqlx::SqlitePool,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let normalized = sql.trim().to_lowercase();

    if let Some(plan) = editable_select_plan(sql) {
        let query = build_editable_paginated_query(
            &plan,
            page_size,
            offset,
            "rowid",
            filter.as_ref(),
            sort.as_ref(),
            SQLITE_DIALECT,
        );
        let rows = sqlx::query(&query)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::Sqlite)?;
        return Ok(QueryOutput::Table(sqlite_preview_rows_to_paginated_page(
            rows,
            plan.source,
            page_size,
            offset,
        )));
    }

    if is_paginated_query(&normalized) {
        let rows = sqlx::query(&build_paginated_query(
            sql,
            page_size,
            offset,
            filter.as_ref(),
            sort.as_ref(),
            SQLITE_DIALECT,
        ))
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::Sqlite)?;
        return Ok(QueryOutput::Table(sqlite_rows_to_paginated_page(
            rows, page_size, offset,
        )));
    }

    if is_tabular_query(&normalized) {
        let rows = sqlx::query(sql)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::Sqlite)?;
        return Ok(QueryOutput::Table(sqlite_rows_to_page(rows)));
    }

    let result = sqlx::query(sql)
        .execute(pool)
        .await
        .map_err(DatabaseError::Sqlite)?;
    Ok(QueryOutput::AffectedRows(result.rows_affected()))
}

async fn execute_postgres_query_page(
    sql: &str,
    pool: &sqlx::PgPool,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let normalized = sql.trim().to_lowercase();

    if let Some(plan) = editable_select_plan(sql) {
        let query = build_editable_paginated_query(
            &plan,
            page_size,
            offset,
            "ctid::text",
            filter.as_ref(),
            sort.as_ref(),
            POSTGRES_DIALECT,
        );
        let rows = sqlx::query(&query)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::Postgres)?;
        return Ok(QueryOutput::Table(postgres_preview_rows_to_paginated_page(
            rows,
            plan.source,
            page_size,
            offset,
        )));
    }

    if is_paginated_query(&normalized) {
        let rows = sqlx::query(&build_paginated_query(
            sql,
            page_size,
            offset,
            filter.as_ref(),
            sort.as_ref(),
            POSTGRES_DIALECT,
        ))
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::Postgres)?;
        return Ok(QueryOutput::Table(postgres_rows_to_paginated_page(
            rows, page_size, offset,
        )));
    }

    if is_tabular_query(&normalized) {
        let rows = sqlx::query(sql)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::Postgres)?;
        return Ok(QueryOutput::Table(postgres_rows_to_page(rows)));
    }

    let result = sqlx::query(sql)
        .execute(pool)
        .await
        .map_err(DatabaseError::Postgres)?;
    Ok(QueryOutput::AffectedRows(result.rows_affected()))
}

async fn execute_mysql_query_page(
    sql: &str,
    pool: &sqlx::MySqlPool,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let normalized = sql.trim().to_lowercase();

    if let Some(plan) = editable_select_plan(sql) {
        let schema_name = mysql_effective_schema_name(pool, plan.source.schema.as_deref()).await?;
        let primary_key_columns =
            mysql_primary_key_columns(pool, &schema_name, &plan.source.table_name).await?;

        if primary_key_columns.is_empty() {
            let rows = sqlx::query(&build_paginated_query(
                sql,
                page_size,
                offset,
                filter.as_ref(),
                sort.as_ref(),
                MYSQL_DIALECT,
            ))
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::MySql)?;
            return Ok(QueryOutput::Table(mysql_rows_to_paginated_page(
                rows, page_size, offset,
            )));
        }

        let locator_expr = mysql_locator_expression(&primary_key_columns);
        let mut plan = plan;
        plan.source.schema = Some(schema_name);
        let query = build_editable_paginated_query(
            &plan,
            page_size,
            offset,
            &locator_expr,
            filter.as_ref(),
            sort.as_ref(),
            MYSQL_DIALECT,
        );
        let rows = sqlx::query(&query)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::MySql)?;
        return Ok(QueryOutput::Table(mysql_preview_rows_to_paginated_page(
            rows,
            plan.source,
            page_size,
            offset,
        )));
    }

    if is_paginated_query(&normalized) {
        let rows = sqlx::query(&build_paginated_query(
            sql,
            page_size,
            offset,
            filter.as_ref(),
            sort.as_ref(),
            MYSQL_DIALECT,
        ))
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::MySql)?;
        return Ok(QueryOutput::Table(mysql_rows_to_paginated_page(
            rows, page_size, offset,
        )));
    }

    if is_tabular_query(&normalized) {
        let rows = sqlx::query(sql)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::MySql)?;
        return Ok(QueryOutput::Table(mysql_rows_to_page(rows)));
    }

    let result = sqlx::query(sql)
        .execute(pool)
        .await
        .map_err(DatabaseError::MySql)?;
    Ok(QueryOutput::AffectedRows(result.rows_affected()))
}

async fn execute_clickhouse_query_page(
    sql: &str,
    config: &models::ClickHouseFormData,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let normalized = sql.trim().to_lowercase();

    if is_paginated_query(&normalized) {
        let response = ClickHouseDriver
            .execute_json_query(
                config,
                &build_paginated_query(
                    sql,
                    page_size,
                    offset,
                    filter.as_ref(),
                    sort.as_ref(),
                    CLICKHOUSE_DIALECT,
                ),
            )
            .await?;
        return Ok(QueryOutput::Table(clickhouse_rows_to_paginated_page(
            response, page_size, offset,
        )));
    }

    if is_tabular_query(&normalized) {
        let response = ClickHouseDriver.execute_json_query(config, sql).await?;
        return Ok(QueryOutput::Table(clickhouse_rows_to_page(response)));
    }

    ClickHouseDriver.execute_text_query(config, sql).await?;
    Ok(QueryOutput::AffectedRows(0))
}

fn is_tabular_query(sql: &str) -> bool {
    is_read_only_sql(sql)
}

fn is_paginated_query(sql: &str) -> bool {
    let keywords = statement_leading_keywords(sql);
    matches!(
        keywords.as_slice(),
        [keyword] if matches!(keyword.as_str(), "select" | "with")
    )
}

fn leading_sql_keyword(sql: &str) -> Option<String> {
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
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_'))
    {
        index += 1;
    }

    (index > start).then(|| sql[start..index].to_ascii_lowercase())
}

fn statement_leading_keywords(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut quote = None::<u8>;

    while index < bytes.len() {
        if let Some(quote_byte) = quote {
            if bytes[index] == quote_byte {
                if quote_byte == b'\'' && index + 1 < bytes.len() && bytes[index + 1] == b'\'' {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            index += 1;
            continue;
        }

        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                quote = Some(bytes[index]);
                index += 1;
            }
            b'-' if index + 1 < bytes.len() && bytes[index + 1] == b'-' => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'*' => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b';' => {
                if let Some(keyword) = leading_sql_keyword(&sql[start..index]) {
                    statements.push(keyword);
                }
                start = index + 1;
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }

    if let Some(keyword) = leading_sql_keyword(&sql[start..]) {
        statements.push(keyword);
    }

    statements
}

fn build_insert_row_sql(
    source: &TablePreviewSource,
    column_values: &[(String, String)],
    quote_identifier_fn: fn(&str) -> String,
) -> String {
    if column_values.is_empty() {
        return format!("insert into {} default values", source.qualified_name);
    }

    let columns = column_values
        .iter()
        .map(|(column_name, _)| quote_identifier_fn(column_name))
        .collect::<Vec<_>>()
        .join(", ");
    let values = column_values
        .iter()
        .map(|(_, value)| sql_literal(value))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "insert into {} ({columns}) values ({values})",
        source.qualified_name
    )
}

fn parse_next_numeric_id(value: String, column_name: &str) -> Result<i64, DatabaseError> {
    value.trim().parse::<i64>().map_err(|_| {
        DatabaseError::UnsupportedDriver(format!(
            "Built-in auto id requires a numeric `{column_name}` column"
        ))
    })
}

async fn sqlite_single_primary_key_column(
    pool: &sqlx::SqlitePool,
    schema_name: &str,
    table_name: &str,
) -> Result<Option<(String, String)>, DatabaseError> {
    let sql = format!(
        "PRAGMA {}.table_info({})",
        quote_identifier(schema_name),
        quote_identifier(table_name)
    );
    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::Sqlite)?;

    let mut primary_key_columns = Vec::new();
    for row in rows {
        let pk_position = row.try_get::<i64, _>("pk").unwrap_or(0);
        if pk_position <= 0 {
            continue;
        }

        let column_name = row
            .try_get::<String, _>("name")
            .map_err(DatabaseError::Sqlite)?;
        let data_type = row
            .try_get::<String, _>("type")
            .unwrap_or_else(|_| String::new());
        primary_key_columns.push((pk_position, column_name, data_type));
    }

    primary_key_columns.sort_by_key(|(pk_position, _, _)| *pk_position);
    if primary_key_columns.len() != 1 {
        return Ok(None);
    }

    let (_, column_name, data_type) = primary_key_columns.remove(0);
    Ok(Some((column_name, data_type)))
}

async fn load_sqlite_create_statement(
    pool: &sqlx::SqlitePool,
    schema_name: &str,
    table_name: &str,
) -> Result<String, DatabaseError> {
    sqlx::query_scalar::<_, Option<String>>(&format!(
        "select sql from {}.sqlite_master where type = 'table' and name = ?1",
        quote_identifier(schema_name)
    ))
    .bind(table_name)
    .fetch_optional(pool)
    .await
    .map_err(DatabaseError::Sqlite)?
    .flatten()
    .filter(|sql| !sql.trim().is_empty())
    .ok_or_else(|| {
        DatabaseError::UnsupportedDriver(format!(
            "Could not load CREATE TABLE statement for {}",
            table_name
        ))
    })
}

async fn load_clickhouse_create_statement(
    config: &models::ClickHouseFormData,
    schema: &Option<String>,
    table_name: &str,
) -> Result<String, DatabaseError> {
    let schema_name = schema
        .as_deref()
        .map(str::trim)
        .filter(|schema| !schema.is_empty())
        .unwrap_or(config.effective_database());
    let sql = format!(
        "SHOW CREATE TABLE {}.{}",
        quote_identifier_clickhouse(schema_name),
        quote_identifier_clickhouse(table_name),
    );
    ClickHouseDriver.execute_text_query(config, &sql).await
}

fn rewrite_create_table_statement(
    create_statement: &str,
    replacement_qualified_name: &str,
) -> Result<String, DatabaseError> {
    let statement = create_statement.trim().trim_end_matches(';').trim();
    let lower = statement.to_ascii_lowercase();
    let create_table = "create table";
    let Some(create_index) = lower.find(create_table) else {
        return Err(DatabaseError::UnsupportedDriver(
            "Could not parse CREATE TABLE statement".to_string(),
        ));
    };

    let mut name_start = create_index + create_table.len();
    name_start = skip_sql_whitespace(statement, name_start);

    let if_not_exists = "if not exists";
    if lower[name_start..].starts_with(if_not_exists) {
        name_start += if_not_exists.len();
        name_start = skip_sql_whitespace(statement, name_start);
    }

    let Some(open_paren_offset) = statement[name_start..].find('(') else {
        return Err(DatabaseError::UnsupportedDriver(
            "Could not find the table definition in CREATE TABLE".to_string(),
        ));
    };
    let definition_start = name_start + open_paren_offset;

    Ok(format!(
        "{}{}{}",
        &statement[..name_start],
        replacement_qualified_name,
        &statement[definition_start..]
    ))
}

fn skip_sql_whitespace(sql: &str, mut index: usize) -> usize {
    while let Some(ch) = sql[index..].chars().next() {
        if ch.is_whitespace() {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    index
}

async fn postgres_single_primary_key_column(
    pool: &sqlx::PgPool,
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
          and tc.table_schema = $1
          and tc.table_name = $2
        order by kcu.ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;

    if rows.len() != 1 {
        return Ok(None);
    }

    let row = &rows[0];
    let column_name = row
        .try_get::<String, _>("column_name")
        .map_err(DatabaseError::Postgres)?;
    let data_type = row
        .try_get::<String, _>("data_type")
        .unwrap_or_else(|_| String::new());
    Ok(Some((column_name, data_type)))
}

#[cfg(test)]
mod round_trip;

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        create_table, drop_table, duplicate_table, execute_query_page, is_read_only_sql,
        leading_sql_keyword, mysql_locator_expression, parse_clickhouse_primary_key_expression,
        parse_mysql_locator, preview_source_for_sql, reorder_clickhouse_primary_key_columns,
        truncate_table,
    };
    use models::{DatabaseConnection, QueryOutput, TablePreviewSource};
    use sqlx::SqlitePool;

    #[test]
    fn read_only_sql_detection_matches_supported_queries() {
        assert!(is_read_only_sql("select * from products"));
        assert!(is_read_only_sql(
            "WITH recent AS (select 1) select * from recent"
        ));
        assert!(is_read_only_sql("show tables"));
        assert!(is_read_only_sql("pragma table_info(products)"));
        assert!(!is_read_only_sql("update products set price = 10"));
        assert!(!is_read_only_sql("delete from products"));
    }

    #[test]
    fn mysql_locator_round_trip_uses_json_array_encoding() {
        let locator = r#"["42","tenant-a"]"#;
        let pk_columns = vec!["id".to_string(), "tenant_id".to_string()];

        assert_eq!(
            mysql_locator_expression(&pk_columns),
            "json_array(cast(`id` as char), cast(`tenant_id` as char))"
        );
        assert_eq!(
            parse_mysql_locator(locator, &pk_columns).unwrap(),
            vec![
                "cast(`id` as char) = '42'",
                "cast(`tenant_id` as char) = 'tenant-a'"
            ]
        );
    }

    #[tokio::test]
    async fn execute_query_page_supports_quoted_sqlite_table_names() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(
            r#"
            create table "products" (
                id integer primary key,
                name text not null,
                price real not null
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            insert into "products" (name, price)
            values
                ('Wireless Mouse', 29.99),
                ('Mechanical Keyboard', 89.99);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = execute_query_page(
            DatabaseConnection::Sqlite(pool),
            r#"select * from "products" limit 100;"#.to_string(),
            100,
            0,
            None,
            None,
        )
        .await
        .unwrap();

        match result {
            QueryOutput::Table(page) => {
                assert_eq!(page.columns, vec!["id", "name", "price"]);
                assert_eq!(page.rows.len(), 2);
                assert_eq!(page.rows[0][1], "Wireless Mouse");
                assert!(page.editable.is_some());
            }
            other => panic!("expected table result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_table_creates_sqlite_table() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        create_table(
            DatabaseConnection::Sqlite(pool.clone()),
            Some("main".to_string()),
            "products".to_string(),
            "id integer primary key,\nname text not null".to_string(),
            None,
        )
        .await
        .unwrap();

        let remaining = sqlx::query_scalar::<_, i64>(
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn drop_table_removes_sqlite_table() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        drop_table(
            DatabaseConnection::Sqlite(pool.clone()),
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
        )
        .await
        .unwrap();

        let remaining = sqlx::query_scalar::<_, i64>(
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn truncate_table_clears_sqlite_rows_without_dropping_table() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(r#"insert into "products" (name) values ('Keyboard'), ('Mouse');"#)
            .execute(&pool)
            .await
            .unwrap();

        truncate_table(
            DatabaseConnection::Sqlite(pool.clone()),
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
        )
        .await
        .unwrap();

        let remaining_rows = sqlx::query_scalar::<_, i64>(r#"select count(*) from "products""#)
            .fetch_one(&pool)
            .await
            .unwrap();
        let remaining_tables = sqlx::query_scalar::<_, i64>(
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(remaining_rows, 0);
        assert_eq!(remaining_tables, 1);
    }

    #[tokio::test]
    async fn duplicate_table_creates_sqlite_copy_with_rows() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(r#"insert into "products" (name) values ('Keyboard'), ('Mouse');"#)
            .execute(&pool)
            .await
            .unwrap();

        duplicate_table(
            DatabaseConnection::Sqlite(pool.clone()),
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
            "products_copy".to_string(),
            true,
        )
        .await
        .unwrap();

        let copy_rows = sqlx::query_scalar::<_, i64>(r#"select count(*) from "products_copy""#)
            .fetch_one(&pool)
            .await
            .unwrap();
        let copied_create_sql = sqlx::query_scalar::<_, Option<String>>(
            r#"
            select sql
            from sqlite_master
            where type = 'table'
              and name = 'products_copy'
            "#,
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .unwrap();

        assert_eq!(copy_rows, 2);
        assert!(copied_create_sql.contains("products_copy"));
    }

    #[tokio::test]
    async fn duplicate_table_can_copy_structure_only_for_sqlite() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();

        sqlx::query(
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(r#"insert into "products" (name) values ('Keyboard');"#)
            .execute(&pool)
            .await
            .unwrap();

        duplicate_table(
            DatabaseConnection::Sqlite(pool.clone()),
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
            "products_empty_copy".to_string(),
            false,
        )
        .await
        .unwrap();

        let copy_rows =
            sqlx::query_scalar::<_, i64>(r#"select count(*) from "products_empty_copy""#)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(copy_rows, 0);
    }

    #[test]
    fn parses_clickhouse_primary_key_expression_in_declared_order() {
        assert_eq!(
            parse_clickhouse_primary_key_expression("tuple(created_at, id)"),
            vec!["created_at", "id"]
        );
        assert_eq!(
            parse_clickhouse_primary_key_expression("`event id`, shard"),
            vec!["event id", "shard"]
        );
    }

    #[test]
    fn reorders_clickhouse_primary_key_columns_from_primary_key_expression() {
        let pk_columns = vec![
            ("id".to_string(), "UInt64".to_string()),
            ("created_at".to_string(), "DateTime".to_string()),
        ];

        assert_eq!(
            reorder_clickhouse_primary_key_columns(pk_columns, "tuple(created_at, id)"),
            vec![
                ("created_at".to_string(), "DateTime".to_string()),
                ("id".to_string(), "UInt64".to_string()),
            ]
        );
    }

    #[test]
    fn keeps_table_order_when_clickhouse_primary_key_expression_is_not_plain_columns() {
        let pk_columns = vec![
            ("created_at".to_string(), "DateTime".to_string()),
            ("id".to_string(), "UInt64".to_string()),
        ];

        assert_eq!(
            reorder_clickhouse_primary_key_columns(
                pk_columns.clone(),
                "tuple(toDate(created_at), id)"
            ),
            pk_columns
        );
    }

    #[test]
    fn infers_preview_source_for_simple_select() {
        let source = preview_source_for_sql(r#"select id, name from "main"."products" limit 100"#)
            .expect("source");

        assert_eq!(source.schema.as_deref(), Some("main"));
        assert_eq!(source.table_name, "products");
        assert_eq!(source.qualified_name, r#""main"."products""#);
    }

    #[test]
    fn skips_preview_source_for_join_query() {
        assert!(
            preview_source_for_sql(
                "select p.id from products p join categories c on c.id = p.category_id"
            )
            .is_none()
        );
    }

    #[test]
    fn leading_keyword_extracts_first_sql_word() {
        assert_eq!(leading_sql_keyword("SELECT 1"), Some("select".to_string()));
        assert_eq!(
            leading_sql_keyword("insert into t values (1)"),
            Some("insert".to_string())
        );
        assert_eq!(
            leading_sql_keyword("  update t set x = 1"),
            Some("update".to_string())
        );
        assert_eq!(
            leading_sql_keyword("-- comment\nselect 1"),
            Some("select".to_string())
        );
        assert_eq!(
            leading_sql_keyword("/* comment */\nselect 1"),
            Some("select".to_string())
        );
        assert_eq!(leading_sql_keyword(""), None);
        assert_eq!(leading_sql_keyword("   "), None);
    }

    #[test]
    fn is_read_only_sql_gates_dispatch_for_keyboard_shortcut_triggers() {
        assert!(is_read_only_sql("select * from users"));
        assert!(is_read_only_sql("explain select * from users"));
        assert!(is_read_only_sql("describe users"));
        assert!(is_read_only_sql("show tables"));
        assert!(is_read_only_sql("WITH cte AS (select 1) select * from cte"));
        assert!(is_read_only_sql("pragma table_info(users)"));
        assert!(!is_read_only_sql(
            "insert into users (name) values ('test')"
        ));
        assert!(!is_read_only_sql("update users set name = 'test'"));
        assert!(!is_read_only_sql("delete from users"));
        assert!(!is_read_only_sql("drop table users"));
        assert!(!is_read_only_sql("alter table users add column email text"));
        assert!(!is_read_only_sql("select 1; drop table users"));
        assert!(is_read_only_sql("select '; drop table users' as text"));
    }
}

fn sqlite_type_supports_auto_id(data_type: &str) -> bool {
    data_type.to_ascii_lowercase().contains("int")
}

fn postgres_type_supports_auto_id(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "smallint" | "integer" | "bigint"
    )
}

fn mysql_type_supports_auto_id(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint"
    )
}

fn clickhouse_type_supports_auto_id(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "int8" | "int16" | "int32" | "int64" | "uint8" | "uint16" | "uint32" | "uint64"
    )
}

fn qualified_sqlite_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema),
            quote_identifier(table_name)
        ),
        None => quote_identifier(table_name),
    }
}

fn qualified_postgres_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema),
            quote_identifier(table_name)
        ),
        None => quote_identifier(table_name),
    }
}

fn qualified_mysql_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier_clickhouse(schema),
            quote_identifier_clickhouse(table_name)
        ),
        None => quote_identifier_clickhouse(table_name),
    }
}

fn qualified_clickhouse_table_name(
    schema: Option<&str>,
    table_name: &str,
    fallback_database: &str,
) -> String {
    let schema = schema
        .map(str::trim)
        .filter(|schema| !schema.is_empty())
        .unwrap_or(fallback_database);
    format!(
        "{}.{}",
        quote_identifier_clickhouse(schema),
        quote_identifier_clickhouse(table_name)
    )
}

async fn clickhouse_get_primary_key_columns(
    config: &models::ClickHouseFormData,
    schema_name: &str,
    table_name: &str,
) -> Result<Option<(Vec<String>, String)>, DatabaseError> {
    let primary_key_expression_sql = format!(
        "SELECT primary_key FROM system.tables \
         WHERE database = {} AND name = {} \
         LIMIT 1",
        sql_literal(schema_name),
        sql_literal(table_name)
    );
    let primary_key_expression = ClickHouseDriver
        .execute_json_query(config, &primary_key_expression_sql)
        .await?
        .data
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next())
        .map(|value| clickhouse_json_value_to_string(&value))
        .unwrap_or_default();

    let columns_sql = format!(
        "SELECT name, type, is_in_primary_key FROM system.columns \
         WHERE database = {} AND table = {} \
         ORDER BY position",
        sql_literal(schema_name),
        sql_literal(table_name)
    );
    let response = ClickHouseDriver
        .execute_json_query(config, &columns_sql)
        .await?;

    let mut pk_columns = Vec::new();
    for row in response.data {
        if row.len() < 3 {
            continue;
        }

        let is_in_primary_key = clickhouse_json_value_to_string(&row[2]) == "1";
        if is_in_primary_key {
            pk_columns.push((
                clickhouse_json_value_to_string(&row[0]),
                clickhouse_json_value_to_string(&row[1]),
            ));
        }
    }

    if pk_columns.is_empty() {
        return Ok(None);
    }

    let pk_columns = reorder_clickhouse_primary_key_columns(pk_columns, &primary_key_expression);
    let first_type = pk_columns
        .first()
        .map(|(_, data_type)| data_type.clone())
        .unwrap_or_default();
    let pk_column_names = pk_columns
        .into_iter()
        .map(|(column_name, _)| column_name)
        .collect();

    Ok(Some((pk_column_names, first_type)))
}

async fn mysql_effective_schema_name(
    pool: &sqlx::MySqlPool,
    schema: Option<&str>,
) -> Result<String, DatabaseError> {
    if let Some(schema) = schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        return Ok(schema.to_string());
    }

    sqlx::query_scalar::<_, Option<String>>("select database()")
        .fetch_one(pool)
        .await
        .map_err(DatabaseError::MySql)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            DatabaseError::UnsupportedDriver(
                "No MySQL database selected. Set a default database or use a qualified table name."
                    .to_string(),
            )
        })
}

async fn mysql_primary_key_columns(
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
    .map_err(DatabaseError::MySql)?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("column_name")
                .map_err(DatabaseError::MySql)
        })
        .collect()
}

async fn mysql_single_primary_key_column(
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
    .map_err(DatabaseError::MySql)?;

    if rows.len() != 1 {
        return Ok(None);
    }

    let row = &rows[0];
    let column_name = row
        .try_get::<String, _>("column_name")
        .map_err(DatabaseError::MySql)?;
    let data_type = row
        .try_get::<String, _>("data_type")
        .unwrap_or_else(|_| String::new());
    Ok(Some((column_name, data_type)))
}

fn mysql_locator_expression(pk_columns: &[String]) -> String {
    let args = pk_columns
        .iter()
        .map(|column| format!("cast({} as char)", quote_identifier_clickhouse(column)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("json_array({args})")
}

fn parse_mysql_locator(locator: &str, pk_columns: &[String]) -> Result<Vec<String>, DatabaseError> {
    let values = serde_json::from_str::<Vec<String>>(locator)
        .map_err(|_| DatabaseError::UnsupportedDriver("Invalid MySQL row locator".to_string()))?;

    if values.len() != pk_columns.len() {
        return Err(DatabaseError::UnsupportedDriver(
            "Invalid MySQL row locator".to_string(),
        ));
    }

    Ok(pk_columns
        .iter()
        .zip(values)
        .map(|(column, value)| {
            format!(
                "cast({} as char) = {}",
                quote_identifier_clickhouse(column),
                sql_literal(&value)
            )
        })
        .collect())
}

fn reorder_clickhouse_primary_key_columns(
    pk_columns: Vec<(String, String)>,
    primary_key_expression: &str,
) -> Vec<(String, String)> {
    let parsed_order = parse_clickhouse_primary_key_expression(primary_key_expression);
    if parsed_order.len() != pk_columns.len() {
        return pk_columns;
    }

    let original = pk_columns.clone();
    let mut remaining = pk_columns;
    let mut ordered = Vec::with_capacity(remaining.len());

    for column_name in parsed_order {
        let Some(index) = remaining.iter().position(|(name, _)| *name == column_name) else {
            return original;
        };
        ordered.push(remaining.remove(index));
    }

    ordered.extend(remaining);
    ordered
}

fn parse_clickhouse_primary_key_expression(expression: &str) -> Vec<String> {
    let expression = expression.trim();
    if expression.is_empty() || expression.eq_ignore_ascii_case("tuple()") {
        return Vec::new();
    }

    let expression = strip_clickhouse_tuple_wrapper(expression).unwrap_or(expression);
    split_clickhouse_expression_list(expression)
        .into_iter()
        .filter_map(parse_clickhouse_identifier_expression)
        .collect()
}

fn strip_clickhouse_tuple_wrapper(expression: &str) -> Option<&str> {
    let expression = expression.trim();
    if !expression
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("tuple"))
    {
        return None;
    }

    let open_index = expression.find('(')?;
    if open_index != 5 || !expression.ends_with(')') {
        return None;
    }

    Some(&expression[(open_index + 1)..(expression.len() - 1)])
}

fn split_clickhouse_expression_list(expression: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_backticks = false;
    let mut in_double_quotes = false;

    for (index, ch) in expression.char_indices() {
        match ch {
            '`' if !in_double_quotes => in_backticks = !in_backticks,
            '"' if !in_backticks => in_double_quotes = !in_double_quotes,
            '(' if !in_backticks && !in_double_quotes => depth += 1,
            ')' if !in_backticks && !in_double_quotes && depth > 0 => depth -= 1,
            ',' if !in_backticks && !in_double_quotes && depth == 0 => {
                segments.push(expression[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    segments.push(expression[start..].trim());
    segments
        .into_iter()
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn parse_clickhouse_identifier_expression(expression: &str) -> Option<String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }

    if expression.starts_with('`') && expression.ends_with('`') && expression.len() >= 2 {
        return Some(expression[1..(expression.len() - 1)].replace("``", "`"));
    }

    if expression.starts_with('"') && expression.ends_with('"') && expression.len() >= 2 {
        return Some(expression[1..(expression.len() - 1)].replace("\"\"", "\""));
    }

    expression
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        .then(|| expression.to_string())
}

fn clickhouse_json_value_to_string_for_pk(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => format!("'{}'", v.replace('\'', "''")),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "NULL".to_string()),
    }
}

fn build_clickhouse_locator(pk_columns: &[String], row_values: &[serde_json::Value]) -> String {
    let mut parts = Vec::new();
    for (col, val) in pk_columns.iter().zip(row_values.iter()) {
        let encoded_col = col.replace('`', "``");
        let encoded_val = clickhouse_json_value_to_string_for_pk(val);
        parts.push(format!("{}={}", encoded_col, encoded_val));
    }
    parts.join("|")
}

fn parse_clickhouse_locator(locator: &str, _pk_columns: &[String]) -> Vec<(String, String)> {
    let parts: Vec<&str> = locator.split('|').collect();
    let mut result = Vec::new();
    for part in parts {
        if let Some((col, val)) = part.split_once('=') {
            let col_decoded = col.replace("``", "`");
            result.push((col_decoded, val.to_string()));
        }
    }
    result
}

fn clickhouse_json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unsupported>".to_string()),
    }
}
