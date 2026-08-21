use database::DatabaseDriver;
use driver_clickhouse::ClickHouseDriver;
use models::{DatabaseConnection, DatabaseError, TablePreviewSource};
use sqlx::Row;

use super::{
    build_insert_row_sql, clickhouse_get_primary_key_columns, clickhouse_type_supports_auto_id,
    invalid_sqlite_locator, mysql_effective_schema_name, mysql_primary_key_columns,
    mysql_single_primary_key_column, mysql_type_supports_auto_id, parse_clickhouse_locator,
    parse_mysql_locator, parse_next_numeric_id, postgres_single_primary_key_column,
    postgres_type_supports_auto_id, quote_identifier, quote_identifier_clickhouse, sql_literal,
    sqlite_single_primary_key_column, sqlite_type_supports_auto_id,
};

pub async fn update_table_cell(
    connection: DatabaseConnection,
    source: TablePreviewSource,
    locator: String,
    column_name: String,
    value: String,
) -> Result<(), DatabaseError> {
    let column = quote_identifier(&column_name);
    let value_literal = sql_literal(&value);

    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let rowid = locator
                .parse::<i64>()
                .map_err(|_| invalid_sqlite_locator())?;
            let sql = format!(
                "update {} set {} = {} where rowid = {}",
                source.qualified_name, column, value_literal, rowid
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = format!(
                "update {} set {} = {} where ctid = {}::tid",
                source.qualified_name,
                column,
                value_literal,
                sql_literal(&locator)
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let schema_name = mysql_effective_schema_name(&pool, source.schema.as_deref()).await?;
            let primary_key_columns =
                mysql_primary_key_columns(&pool, &schema_name, &source.table_name).await?;
            if primary_key_columns.is_empty() {
                return Err(DatabaseError::UnsupportedDriver(
                    "MySQL table must have a primary key for updates".to_string(),
                ));
            }

            let conditions = parse_mysql_locator(&locator, &primary_key_columns)?;
            let where_clause = conditions.join(" AND ");
            let column = quote_identifier_clickhouse(&column_name);
            let sql = format!(
                "update {} set {} = {} where {}",
                source.qualified_name, column, value_literal, where_clause
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let pk_result =
                clickhouse_get_primary_key_columns(&config, &schema_name, &source.table_name)
                    .await?;

            let Some((pk_columns, _)) = pk_result else {
                return Err(DatabaseError::UnsupportedDriver(
                    "ClickHouse table must have a primary key for updates".to_string(),
                ));
            };

            let conditions = parse_clickhouse_locator(&locator, &pk_columns);
            if conditions.is_empty() {
                return Err(DatabaseError::UnsupportedDriver(
                    "Invalid row locator".to_string(),
                ));
            }

            let where_clause = conditions
                .iter()
                .map(|(col, val)| format!("{} = {}", quote_identifier_clickhouse(col), val))
                .collect::<Vec<_>>()
                .join(" AND ");

            let column = quote_identifier_clickhouse(&column_name);
            let value_literal = sql_literal(&value);

            let sql = format!(
                "ALTER TABLE {} UPDATE {} = {} WHERE {}",
                source.qualified_name, column, value_literal, where_clause
            );

            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}

pub async fn insert_table_row(
    connection: DatabaseConnection,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let sql = format!("insert into {} default values", source.qualified_name);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = format!("insert into {} default values", source.qualified_name);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let sql = format!("insert into {} values ()", source.qualified_name);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(_) => Err(DatabaseError::UnsupportedDriver(
            "ClickHouse row inserts are not supported yet".to_string(),
        )),
    }
}

pub async fn insert_table_row_with_values(
    connection: DatabaseConnection,
    source: TablePreviewSource,
    column_values: Vec<(String, String)>,
) -> Result<(), DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let sql = build_insert_row_sql(&source, &column_values, quote_identifier);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = build_insert_row_sql(&source, &column_values, quote_identifier);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let sql = build_insert_row_sql(&source, &column_values, quote_identifier_clickhouse);
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let sql = build_insert_row_sql(&source, &column_values, quote_identifier_clickhouse);

            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}

pub async fn next_table_primary_key_id(
    connection: DatabaseConnection,
    source: TablePreviewSource,
) -> Result<Option<(String, i64)>, DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let schema_name = source.schema.clone().unwrap_or_else(|| "main".to_string());
            let Some((column_name, data_type)) =
                sqlite_single_primary_key_column(&pool, &schema_name, &source.table_name).await?
            else {
                return Ok(None);
            };
            if !sqlite_type_supports_auto_id(&data_type) {
                return Ok(None);
            }

            let column = quote_identifier(&column_name);
            let sql = format!(
                "select cast(coalesce(max({column}), 0) + 1 as text) from {}",
                source.qualified_name
            );
            let row = sqlx::query(&sql)
                .fetch_one(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(Some((
                column_name.clone(),
                parse_next_numeric_id(
                    row.try_get::<String, _>(0).map_err(DatabaseError::Sqlite)?,
                    &column_name,
                )?,
            )))
        }
        DatabaseConnection::Postgres(pool) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "public".to_string());
            let Some((column_name, data_type)) =
                postgres_single_primary_key_column(&pool, &schema_name, &source.table_name).await?
            else {
                return Ok(None);
            };
            if !postgres_type_supports_auto_id(&data_type) {
                return Ok(None);
            }

            let column = quote_identifier(&column_name);
            let sql = format!(
                "select cast(coalesce(max({column})::bigint, 0) + 1 as text) from {}",
                source.qualified_name
            );
            let row = sqlx::query(&sql)
                .fetch_one(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(Some((
                column_name.clone(),
                parse_next_numeric_id(
                    row.try_get::<String, _>(0)
                        .map_err(DatabaseError::Postgres)?,
                    &column_name,
                )?,
            )))
        }
        DatabaseConnection::MySql(pool) => {
            let schema_name = mysql_effective_schema_name(&pool, source.schema.as_deref()).await?;
            let Some((column_name, data_type)) =
                mysql_single_primary_key_column(&pool, &schema_name, &source.table_name).await?
            else {
                return Ok(None);
            };
            if !mysql_type_supports_auto_id(&data_type) {
                return Ok(None);
            }

            let column = quote_identifier_clickhouse(&column_name);
            let sql = format!(
                "select cast(coalesce(max({column}), 0) + 1 as char) from {}",
                source.qualified_name
            );
            let row = sqlx::query(&sql)
                .fetch_one(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(Some((
                column_name.clone(),
                parse_next_numeric_id(
                    row.try_get::<String, _>(0).map_err(DatabaseError::MySql)?,
                    &column_name,
                )?,
            )))
        }
        DatabaseConnection::ClickHouse(config) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let pk_result =
                clickhouse_get_primary_key_columns(&config, &schema_name, &source.table_name)
                    .await?;

            let Some((pk_columns, data_type)) = pk_result else {
                return Ok(None);
            };

            if !clickhouse_type_supports_auto_id(&data_type) {
                return Ok(None);
            }

            let column = quote_identifier_clickhouse(&pk_columns[0]);
            let sql = format!(
                "SELECT toString(COALESCE(MAX({}), 0) + 1) AS next_id FROM {}",
                column, source.qualified_name
            );
            let response = ClickHouseDriver.execute_json_query(&config, &sql).await?;

            if let Some(row) = response.data.first()
                && let Some(val) = row.first()
            {
                let next_id = match val {
                    serde_json::Value::String(s) => s.parse::<i64>().map_err(|e| {
                        DatabaseError::ClickHouse(format!(
                            "next_table_primary_key_id: failed to parse string '{s}' as i64: {e}"
                        ))
                    })?,
                    serde_json::Value::Number(n) => n.as_i64().ok_or_else(|| {
                        DatabaseError::ClickHouse(format!(
                            "next_table_primary_key_id: number is not a valid i64: {n}"
                        ))
                    })?,
                    other => {
                        return Err(DatabaseError::ClickHouse(format!(
                            "next_table_primary_key_id: unexpected value type {:?}",
                            other
                        )));
                    }
                };
                Ok(Some((pk_columns[0].clone(), next_id)))
            } else {
                Ok(None)
            }
        }
    }
}

pub async fn delete_table_row(
    connection: DatabaseConnection,
    source: TablePreviewSource,
    locator: String,
) -> Result<(), DatabaseError> {
    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let rowid = locator
                .parse::<i64>()
                .map_err(|_| invalid_sqlite_locator())?;
            let sql = format!(
                "delete from {} where rowid = {}",
                source.qualified_name, rowid
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = format!(
                "delete from {} where ctid = {}::tid",
                source.qualified_name,
                sql_literal(&locator)
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let schema_name = mysql_effective_schema_name(&pool, source.schema.as_deref()).await?;
            let primary_key_columns =
                mysql_primary_key_columns(&pool, &schema_name, &source.table_name).await?;
            if primary_key_columns.is_empty() {
                return Err(DatabaseError::UnsupportedDriver(
                    "MySQL table must have a primary key for deletes".to_string(),
                ));
            }

            let conditions = parse_mysql_locator(&locator, &primary_key_columns)?;
            let where_clause = conditions.join(" AND ");
            let sql = format!(
                "delete from {} where {}",
                source.qualified_name, where_clause
            );
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let schema_name = source
                .schema
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let pk_result =
                clickhouse_get_primary_key_columns(&config, &schema_name, &source.table_name)
                    .await?;

            let Some((pk_columns, _)) = pk_result else {
                return Err(DatabaseError::UnsupportedDriver(
                    "ClickHouse table must have a primary key for deletes".to_string(),
                ));
            };

            let conditions = parse_clickhouse_locator(&locator, &pk_columns);
            if conditions.is_empty() {
                return Err(DatabaseError::UnsupportedDriver(
                    "Invalid row locator".to_string(),
                ));
            }

            let where_clause = conditions
                .iter()
                .map(|(col, val)| format!("{} = {}", quote_identifier_clickhouse(col), val))
                .collect::<Vec<_>>()
                .join(" AND ");

            let sql = format!(
                "ALTER TABLE {} DELETE WHERE {}",
                source.qualified_name, where_clause
            );

            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}
