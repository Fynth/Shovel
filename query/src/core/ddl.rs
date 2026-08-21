use database::DatabaseDriver;
use driver_clickhouse::ClickHouseDriver;
use models::{DatabaseConnection, DatabaseError, TablePreviewSource};

use super::{
    load_clickhouse_create_statement, load_sqlite_create_statement,
    qualified_clickhouse_table_name, qualified_mysql_table_name, qualified_postgres_table_name,
    qualified_sqlite_table_name, rewrite_create_table_statement,
};

pub async fn create_table(
    connection: DatabaseConnection,
    schema: Option<String>,
    table_name: String,
    columns_sql: String,
    clickhouse_engine: Option<String>,
) -> Result<(), DatabaseError> {
    let table_name = table_name.trim();
    let columns_sql = columns_sql.trim().trim_end_matches(';').trim();
    if table_name.is_empty() {
        return Err(DatabaseError::UnsupportedDriver(
            "Table name is empty".to_string(),
        ));
    }
    if columns_sql.is_empty() {
        return Err(DatabaseError::UnsupportedDriver(
            "Table definition is empty".to_string(),
        ));
    }

    let columns_sql = if columns_sql.starts_with('(') {
        columns_sql.to_string()
    } else {
        format!("(\n{columns_sql}\n)")
    };

    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let qualified_name = qualified_sqlite_table_name(schema.as_deref(), table_name);
            let sql = format!("create table {qualified_name} {columns_sql}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let qualified_name = qualified_postgres_table_name(schema.as_deref(), table_name);
            let sql = format!("create table {qualified_name} {columns_sql}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let qualified_name = qualified_mysql_table_name(schema.as_deref(), table_name);
            let sql = format!("create table {qualified_name} {columns_sql}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let engine = clickhouse_engine
                .map(|engine| engine.trim().trim_end_matches(';').trim().to_string())
                .filter(|engine| !engine.is_empty())
                .ok_or_else(|| {
                    DatabaseError::UnsupportedDriver(
                        "ClickHouse engine clause is empty".to_string(),
                    )
                })?;
            let qualified_name = qualified_clickhouse_table_name(
                schema.as_deref(),
                table_name,
                config.effective_database(),
            );
            let sql = format!("create table {qualified_name} {columns_sql} {engine}");
            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}

pub async fn drop_table(
    connection: DatabaseConnection,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    let sql = format!(
        "drop table if exists {}",
        source.qualified_name.trim().trim_end_matches(';')
    );

    match connection {
        DatabaseConnection::Sqlite(pool) => {
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}

pub async fn truncate_table(
    connection: DatabaseConnection,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    let qualified_name = source.qualified_name.trim().trim_end_matches(';');

    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let sql = format!("delete from {qualified_name}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;
            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let sql = format!("truncate table {qualified_name}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;
            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let sql = format!("truncate table {qualified_name}");
            sqlx::query(&sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;
            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let sql = format!("truncate table {qualified_name}");
            ClickHouseDriver.execute_text_query(&config, &sql).await?;
            Ok(())
        }
    }
}

pub async fn duplicate_table(
    connection: DatabaseConnection,
    source: TablePreviewSource,
    new_table_name: String,
    copy_data: bool,
) -> Result<(), DatabaseError> {
    let new_table_name = new_table_name.trim();
    if new_table_name.is_empty() {
        return Err(DatabaseError::UnsupportedDriver(
            "New table name is empty".to_string(),
        ));
    }
    if new_table_name == source.table_name.trim() {
        return Err(DatabaseError::UnsupportedDriver(
            "New table name must be different from the source table".to_string(),
        ));
    }

    let source_qualified_name = source.qualified_name.trim().trim_end_matches(';');

    match connection {
        DatabaseConnection::Sqlite(pool) => {
            let schema_name = source
                .schema
                .as_deref()
                .map(str::trim)
                .filter(|schema| !schema.is_empty())
                .unwrap_or("main");
            let target_qualified_name =
                qualified_sqlite_table_name(source.schema.as_deref(), new_table_name);
            let create_statement =
                load_sqlite_create_statement(&pool, schema_name, &source.table_name).await?;
            let create_sql =
                rewrite_create_table_statement(&create_statement, &target_qualified_name)?;

            sqlx::query(&create_sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Sqlite)?;

            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                sqlx::query(&insert_sql)
                    .execute(&pool)
                    .await
                    .map_err(DatabaseError::Sqlite)?;
            }

            Ok(())
        }
        DatabaseConnection::Postgres(pool) => {
            let target_qualified_name =
                qualified_postgres_table_name(source.schema.as_deref(), new_table_name);
            let create_sql = format!(
                "create table {target_qualified_name} (like {source_qualified_name} including all)"
            );
            sqlx::query(&create_sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::Postgres)?;

            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                sqlx::query(&insert_sql)
                    .execute(&pool)
                    .await
                    .map_err(DatabaseError::Postgres)?;
            }

            Ok(())
        }
        DatabaseConnection::MySql(pool) => {
            let target_qualified_name =
                qualified_mysql_table_name(source.schema.as_deref(), new_table_name);
            let create_sql =
                format!("create table {target_qualified_name} like {source_qualified_name}");
            sqlx::query(&create_sql)
                .execute(&pool)
                .await
                .map_err(DatabaseError::MySql)?;

            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                sqlx::query(&insert_sql)
                    .execute(&pool)
                    .await
                    .map_err(DatabaseError::MySql)?;
            }

            Ok(())
        }
        DatabaseConnection::ClickHouse(config) => {
            let target_qualified_name = qualified_clickhouse_table_name(
                source.schema.as_deref(),
                new_table_name,
                config.effective_database(),
            );
            let create_statement =
                load_clickhouse_create_statement(&config, &source.schema, &source.table_name)
                    .await?;
            let create_sql =
                rewrite_create_table_statement(&create_statement, &target_qualified_name)?;
            ClickHouseDriver
                .execute_text_query(&config, &create_sql)
                .await?;

            if copy_data {
                let insert_sql = format!(
                    "insert into {target_qualified_name} select * from {source_qualified_name}"
                );
                ClickHouseDriver
                    .execute_text_query(&config, &insert_sql)
                    .await?;
            }

            Ok(())
        }
    }
}
