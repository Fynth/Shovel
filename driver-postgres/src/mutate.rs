// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{MutateExec, quote_ident_double};
use models::{DatabaseError, TablePreviewSource};
use sqlx::Row;

use crate::session::PostgresSession;

const IMPORT_BATCH_SIZE: usize = 200;

#[async_trait]
impl MutateExec for PostgresSession {
    async fn update_table_cell(
        &self,
        source: TablePreviewSource,
        locator: String,
        column_name: String,
        value: String,
    ) -> Result<(), DatabaseError> {
        let sql = format!(
            "update {} set {} = {} where ctid = {}::tid",
            source.qualified_name,
            quote_ident_double(&column_name),
            sql_literal(&value),
            sql_literal(&locator)
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
        let sql = format!(
            "delete from {} where ctid = {}::tid",
            source.qualified_name,
            sql_literal(&locator)
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
        let schema_name = source
            .schema
            .clone()
            .unwrap_or_else(|| "public".to_string());
        let Some((column_name, data_type)) =
            postgres_single_primary_key_column(&self.pool, &schema_name, &source.table_name)
                .await?
        else {
            return Ok(None);
        };
        if !postgres_type_supports_auto_id(&data_type) {
            return Ok(None);
        }

        let column = quote_ident_double(&column_name);
        let sql = format!(
            "select cast(coalesce(max({column})::bigint, 0) + 1 as text) from {}",
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

fn postgres_type_supports_auto_id(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "smallint" | "integer" | "bigint"
    )
}

fn parse_next_numeric_id(value: String, column_name: &str) -> Result<i64, DatabaseError> {
    value.trim().parse::<i64>().map_err(|_| {
        DatabaseError::Unsupported(format!(
            "Built-in auto id requires a numeric `{column_name}` column"
        ))
    })
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
