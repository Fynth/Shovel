use database::DatabaseDriver;
use driver_clickhouse::ClickHouseDriver;
use models::{
    DatabaseConnection,
    DatabaseError,
    QueryFilter,
    QueryOutput,
    QuerySort,
    TablePreviewSource,
};

use super::{
    CLICKHOUSE_DIALECT,
    LOCATOR_COLUMN,
    MYSQL_DIALECT,
    POSTGRES_DIALECT,
    SQLITE_DIALECT,
    build_clickhouse_locator,
    build_outer_paginated_query,
    clickhouse_get_primary_key_columns,
    clickhouse_json_value_to_string,
    mysql_effective_schema_name,
    mysql_locator_expression,
    mysql_primary_key_columns,
    quote_identifier_clickhouse,
    rows::{
        mysql_preview_rows_to_paginated_page,
        mysql_rows_to_paginated_page,
        postgres_preview_rows_to_paginated_page,
        sqlite_preview_rows_to_paginated_page,
    },
};

pub async fn load_table_preview_page(
    connection: DatabaseConnection,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let sql = build_outer_paginated_query(
                format!(
                    r#"select rowid as "{LOCATOR_COLUMN}", * from {}"#,
                    source.qualified_name
                ),
                page_size,
                offset,
                filter.as_ref(),
                sort.as_ref(),
                SQLITE_DIALECT,
            );
            let rows = sqlx::query(&sql)
                .fetch_all(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(QueryOutput::Table(sqlite_preview_rows_to_paginated_page(
                rows, source, page_size, offset,
            )))
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = build_outer_paginated_query(
                format!(
                    r#"select ctid::text as "{LOCATOR_COLUMN}", * from {}"#,
                    source.qualified_name
                ),
                page_size,
                offset,
                filter.as_ref(),
                sort.as_ref(),
                POSTGRES_DIALECT,
            );
            let rows = sqlx::query(&sql)
                .fetch_all(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(QueryOutput::Table(postgres_preview_rows_to_paginated_page(
                rows, source, page_size, offset,
            )))
        }
        DatabaseConnection::MySql(pool) => {
            let schema_name = mysql_effective_schema_name(&pool, source.schema.as_deref()).await?;
            let primary_key_columns =
                mysql_primary_key_columns(&pool, &schema_name, &source.table_name).await?;

            if primary_key_columns.is_empty() {
                let sql = build_outer_paginated_query(
                    format!(r#"select * from {}"#, source.qualified_name),
                    page_size,
                    offset,
                    filter.as_ref(),
                    sort.as_ref(),
                    MYSQL_DIALECT,
                );
                let rows = sqlx::query(&sql)
                    .fetch_all(&pool)
                    .await
                    .map_err(DatabaseError::MySql)?;
                Ok(QueryOutput::Table(mysql_rows_to_paginated_page(
                    rows, page_size, offset,
                )))
            } else {
                let locator_expr = mysql_locator_expression(&primary_key_columns);
                let sql = build_outer_paginated_query(
                    format!(
                        r#"select {locator_expr} as "{LOCATOR_COLUMN}", * from {}"#,
                        source.qualified_name
                    ),
                    page_size,
                    offset,
                    filter.as_ref(),
                    sort.as_ref(),
                    MYSQL_DIALECT,
                );
                let rows = sqlx::query(&sql)
                    .fetch_all(&pool)
                    .await
                    .map_err(DatabaseError::MySql)?;
                let source = models::TablePreviewSource {
                    schema: Some(schema_name),
                    ..source
                };
                Ok(QueryOutput::Table(mysql_preview_rows_to_paginated_page(
                    rows, source, page_size, offset,
                )))
            }
        }
        DatabaseConnection::ClickHouse(config) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let pk_result =
                clickhouse_get_primary_key_columns(&config, &schema_name, &source.table_name)
                    .await?;

            let (response, _row_locators) = if let Some((ref pk_columns, _)) = pk_result {
                let pk_select = pk_columns
                    .iter()
                    .map(|c| quote_identifier_clickhouse(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = build_outer_paginated_query(
                    format!("select {pk_select}, * from {}", source.qualified_name),
                    page_size,
                    offset,
                    filter.as_ref(),
                    sort.as_ref(),
                    CLICKHOUSE_DIALECT,
                );
                let response = ClickHouseDriver.execute_json_query(&config, &sql).await?;

                let pk_count = pk_columns.len();
                let row_locators: Vec<String> = response
                    .data
                    .iter()
                    .map(|row| build_clickhouse_locator(pk_columns, &row[..pk_count]))
                    .collect();
                (response, row_locators)
            } else {
                let sql = build_outer_paginated_query(
                    format!("select * from {}", source.qualified_name),
                    page_size,
                    offset,
                    filter.as_ref(),
                    sort.as_ref(),
                    CLICKHOUSE_DIALECT,
                );
                let response = ClickHouseDriver.execute_json_query(&config, &sql).await?;
                (response, vec![])
            };

            // Product policy: ClickHouse table previews are read-only for now.
            let editable = None;

            let (columns, rows) = if let Some((ref pk_columns, _)) = pk_result {
                let pk_count = pk_columns.len();
                let columns: Vec<String> = response.meta[pk_count..]
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let rows: Vec<Vec<String>> = response
                    .data
                    .iter()
                    .map(|row| {
                        row[pk_count..]
                            .iter()
                            .map(clickhouse_json_value_to_string)
                            .collect()
                    })
                    .collect();
                (columns, rows)
            } else {
                let columns: Vec<String> = response.meta.iter().map(|m| m.name.clone()).collect();
                let rows: Vec<Vec<String>> = response
                    .data
                    .iter()
                    .map(|row| row.iter().map(clickhouse_json_value_to_string).collect())
                    .collect();
                (columns, rows)
            };

            let has_next = response.data.len() > page_size as usize;
            Ok(QueryOutput::Table(models::QueryPage {
                columns,
                rows,
                editable,
                offset,
                page_size,
                has_previous: offset > 0,
                has_next,
            }))
        }
    }
}
