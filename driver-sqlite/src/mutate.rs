// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{MutateExec, quote_ident_double};
use models::{DatabaseError, TablePreviewSource};
use sqlx::Row;

use crate::session::SqliteSession;

const IMPORT_BATCH_SIZE: usize = 200;

#[async_trait]
impl MutateExec for SqliteSession {
    async fn update_table_cell(
        &self,
        source: TablePreviewSource,
        locator: String,
        column_name: String,
        value: String,
    ) -> Result<(), DatabaseError> {
        let rowid = locator
            .parse::<i64>()
            .map_err(|_| invalid_sqlite_locator())?;
        let sql = format!(
            "update {} set {} = {} where rowid = {}",
            source.qualified_name,
            quote_ident_double(&column_name),
            sql_literal(&value),
            rowid
        );
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(())
    }

    async fn insert_table_row(&self, source: TablePreviewSource) -> Result<(), DatabaseError> {
        let sql = format!("insert into {} default values", source.qualified_name);
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(())
    }

    async fn insert_table_row_with_values(
        &self,
        source: TablePreviewSource,
        column_values: Vec<(String, String)>,
    ) -> Result<(), DatabaseError> {
        let sql = build_insert_row_sql(&source, &column_values, quote_ident_double);
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(())
    }

    async fn delete_table_row(
        &self,
        source: TablePreviewSource,
        locator: String,
    ) -> Result<(), DatabaseError> {
        let rowid = locator
            .parse::<i64>()
            .map_err(|_| invalid_sqlite_locator())?;
        let sql = format!(
            "delete from {} where rowid = {}",
            source.qualified_name, rowid
        );
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(())
    }

    async fn next_table_primary_key_id(
        &self,
        source: TablePreviewSource,
    ) -> Result<Option<(String, i64)>, DatabaseError> {
        let schema_name = source.schema.clone().unwrap_or_else(|| "main".to_string());
        let Some((column_name, data_type)) =
            sqlite_single_primary_key_column(&self.pool, &schema_name, &source.table_name).await?
        else {
            return Ok(None);
        };
        if !sqlite_type_supports_auto_id(&data_type) {
            return Ok(None);
        }

        let column = quote_ident_double(&column_name);
        let sql = format!(
            "select cast(coalesce(max({column}), 0) + 1 as text) from {}",
            source.qualified_name
        );
        let row = sqlx::query(&sql)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(Some((
            column_name.clone(),
            parse_next_numeric_id(
                row.try_get::<String, _>(0)
                    .map_err(|e| DatabaseError::Driver(e.to_string()))?,
                &column_name,
            )?,
        )))
    }

    async fn import_csv(
        &self,
        source: TablePreviewSource,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> Result<u64, DatabaseError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;

        for chunk in rows.chunks(IMPORT_BATCH_SIZE) {
            let sql = build_insert_sql(&source, &headers, chunk, quote_ident_double);
            sqlx::query(&sql)
                .execute(&mut *transaction)
                .await
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        Ok(rows.len() as u64)
    }
}

fn invalid_sqlite_locator() -> DatabaseError {
    DatabaseError::Unsupported("invalid SQLite row locator".to_string())
}

fn sqlite_type_supports_auto_id(data_type: &str) -> bool {
    data_type.to_ascii_lowercase().contains("int")
}

fn parse_next_numeric_id(value: String, column_name: &str) -> Result<i64, DatabaseError> {
    value.trim().parse::<i64>().map_err(|_| {
        DatabaseError::Unsupported(format!(
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
        quote_ident_double(schema_name),
        quote_ident_double(table_name)
    );
    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut primary_key_columns = Vec::new();
    for row in rows {
        let pk_position = row.try_get::<i64, _>("pk").unwrap_or(0);
        if pk_position <= 0 {
            continue;
        }

        let column_name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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

fn build_insert_sql(
    source: &TablePreviewSource,
    headers: &[String],
    rows: &[Vec<String>],
    quote_identifier_fn: fn(&str) -> String,
) -> String {
    let columns = headers
        .iter()
        .map(|header| quote_identifier_fn(header))
        .collect::<Vec<_>>()
        .join(", ");
    let values = rows
        .iter()
        .map(|row| {
            let cells = row
                .iter()
                .map(|value| import_sql_literal(value))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({cells})")
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "insert into {} ({columns}) values {values}",
        source.qualified_name
    )
}

fn sql_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("null") {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn import_sql_literal(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("null") || trimmed == "\\N" {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}
