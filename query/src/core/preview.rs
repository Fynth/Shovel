use database::{DatabaseDriver, LiveConnection};
use driver_clickhouse::ClickHouseDriver;
use models::{DatabaseError, QueryFilter, QueryOutput, QuerySort, TablePreviewSource};

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
    connection: LiveConnection,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    match connection {
        LiveConnection::Sqlite(pool) => {
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
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
            Ok(QueryOutput::Table(sqlite_preview_rows_to_paginated_page(
                rows, source, page_size, offset,
            )))
        }
        LiveConnection::Postgres(pool) => {
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
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
            Ok(QueryOutput::Table(postgres_preview_rows_to_paginated_page(
                rows, source, page_size, offset,
            )))
        }
        LiveConnection::MySql(pool) => {
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
                    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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
                    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
                let source = models::TablePreviewSource {
                    schema: Some(schema_name),
                    ..source
                };
                Ok(QueryOutput::Table(mysql_preview_rows_to_paginated_page(
                    rows, source, page_size, offset,
                )))
            }
        }
        LiveConnection::ClickHouse(config) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let pk_result =
                clickhouse_get_primary_key_columns(&config, &schema_name, &source.table_name)
                    .await?;

            let (response, row_locators) = if let Some((ref pk_columns, _)) = pk_result {
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

                let row_locators = clickhouse_row_locators(pk_columns, &response.data);
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

            // ClickHouse table previews are editable when the table has a
            // primary key, because mutations rely on primary-key row locators.
            let editable = clickhouse_editable_context(&pk_result, source, row_locators);

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

/// Builds the `col=value|col=value` locator string for every row, taking only
/// the leading PK columns of each row (the query emits `pk_select, *`).
fn clickhouse_row_locators(pk_columns: &[String], data: &[Vec<serde_json::Value>]) -> Vec<String> {
    let pk_count = pk_columns.len();
    data.iter()
        .map(|row| build_clickhouse_locator(pk_columns, &row[..pk_count]))
        .collect()
}

/// Gates preview editability on the presence of a primary key: mutations
/// (insert/update/delete) require primary-key row locators, so a pk-less
/// ClickHouse table stays read-only (`None`).
fn clickhouse_editable_context(
    pk_result: &Option<(Vec<String>, String)>,
    source: TablePreviewSource,
    row_locators: Vec<String>,
) -> Option<models::EditableTableContext> {
    pk_result.as_ref().map(|_| models::EditableTableContext {
        source,
        row_locators,
    })
}

#[cfg(test)]
mod tests {
    use super::{clickhouse_editable_context, clickhouse_row_locators};
    use models::TablePreviewSource;

    #[test]
    fn clickhouse_locators_use_leading_pk_columns_only() {
        let pk_columns = vec!["id".to_string(), "tenant_id".to_string()];
        let data = vec![
            vec![
                serde_json::json!(1),
                serde_json::json!("tenant-a"),
                serde_json::json!("alpha"),
                serde_json::json!(10),
            ],
            vec![
                serde_json::json!(2),
                serde_json::json!("tenant-b"),
                serde_json::json!("beta"),
                serde_json::json!(20),
            ],
        ];

        let locators = clickhouse_row_locators(&pk_columns, &data);

        assert_eq!(
            locators,
            vec!["id=1|tenant_id='tenant-a'", "id=2|tenant_id='tenant-b'"]
        );
    }

    #[test]
    fn clickhouse_locators_tolerate_missing_pk_values() {
        let pk_columns = vec!["id".to_string(), "note".to_string()];
        let data = vec![vec![
            serde_json::json!(7),
            serde_json::Value::Null,
            serde_json::json!("payload"),
        ]];

        assert_eq!(
            clickhouse_row_locators(&pk_columns, &data),
            vec!["id=7|note=NULL"]
        );
    }

    fn source() -> TablePreviewSource {
        TablePreviewSource {
            schema: Some("analytics".to_string()),
            table_name: "events".to_string(),
            qualified_name: "analytics.events".to_string(),
        }
    }

    #[test]
    fn clickhouse_pk_table_exposes_editable_context_with_locators() {
        let pk_result = Some((vec!["id".to_string()], "UInt64".to_string()));
        let locators = vec!["id=1".to_string(), "id=2".to_string()];

        let editable = clickhouse_editable_context(&pk_result, source(), locators.clone());

        assert_eq!(
            editable,
            Some(models::EditableTableContext {
                source: source(),
                row_locators: locators,
            })
        );
    }

    #[test]
    fn clickhouse_pk_less_table_stays_read_only() {
        let editable = clickhouse_editable_context(&None, source(), vec![]);

        assert_eq!(editable, None);
    }
}
