// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::IntrospectExec;
use models::{
    ColumnInfo,
    IndexInfo,
    IndexStat,
    IntrospectionResult,
    LockInfo,
    SchemaInfo,
    TableInfo,
    TableStat,
};
use sqlx::Row;

use crate::session::SqliteSession;

#[async_trait]
impl IntrospectExec for SqliteSession {
    async fn introspect(&self) -> IntrospectionResult {
        let collected_at = Some(unix_secs());
        IntrospectionResult {
            locks: collect_locks(&self.pool).await.unwrap_or_default(),
            index_stats: collect_index_stats(&self.pool).await.unwrap_or_default(),
            table_stats: collect_table_stats(&self.pool).await.unwrap_or_default(),
            schema_info: collect_schema_info(&self.pool).await.unwrap_or_default(),
            collected_at,
            ..Default::default()
        }
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn collect_locks(pool: &sqlx::SqlitePool) -> Result<Vec<LockInfo>, String> {
    let rows = sqlx::query("PRAGMA wal_checkpoint")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to collect SQLite locks: {e}"))?;

    let mut locks = Vec::new();
    for row in rows {
        locks.push(LockInfo {
            database: "main".to_string(),
            relation: None,
            mode: "WAL".to_string(),
            granted: true,
            query: None,
            pid: None,
            wait_start: row.try_get("busy").ok(),
        });
    }
    Ok(locks)
}

async fn collect_index_stats(pool: &sqlx::SqlitePool) -> Result<Vec<IndexStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                'main' as schema,
                tbl_name as table,
                name as index_name
            FROM sqlite_master
            WHERE type = 'index'
            ORDER BY tbl_name, name
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect SQLite index stats: {e}"))?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(IndexStat {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            index_name: row.try_get::<String, _>("index_name").unwrap_or_default(),
            idx_scan: 0,
            idx_tup_read: 0,
            idx_tup_fetch: 0,
        });
    }
    Ok(stats)
}

async fn collect_table_stats(pool: &sqlx::SqlitePool) -> Result<Vec<TableStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                'main' as schema,
                name as table,
                (SELECT COUNT(*) FROM pragma_table_info(t.name)) as column_count
            FROM sqlite_master t
            WHERE type = 'table'
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect SQLite table stats: {e}"))?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(TableStat {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            seq_scan: 0,
            seq_tup_read: 0,
            idx_scan: 0,
            idx_tup_fetch: 0,
            n_tup_ins: 0,
            n_tup_upd: 0,
            n_tup_del: 0,
            n_live_tup: 0,
            n_dead_tup: 0,
        });
    }
    Ok(stats)
}

async fn collect_schema_info(pool: &sqlx::SqlitePool) -> Result<SchemaInfo, String> {
    let mut schema_info = SchemaInfo::default();

    let rows = sqlx::query(
        r#"
            SELECT
                'main' as schema,
                name
            FROM sqlite_master
            WHERE type = 'table'
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect SQLite schema info: {e}"))?;

    for row in rows {
        let schema: String = row.try_get("schema").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();

        let column_rows = sqlx::query(&format!("PRAGMA table_info(\"{name}\")"))
            .fetch_all(pool)
            .await
            .unwrap_or_default();

        let mut columns = Vec::new();
        for col_row in column_rows {
            columns.push(ColumnInfo {
                name: col_row.try_get::<String, _>("name").unwrap_or_default(),
                data_type: col_row.try_get::<String, _>("type").unwrap_or_default(),
                nullable: col_row
                    .try_get::<i32, _>("notnull")
                    .map(|n| n == 0)
                    .unwrap_or(true),
                default: col_row.try_get("dflt_value").ok(),
            });
        }

        schema_info.tables.push(TableInfo {
            schema,
            name,
            columns,
        });
    }

    let index_rows = sqlx::query(
        r#"
            SELECT
                'main' as schema,
                tbl_name as table,
                name,
                sql
            FROM sqlite_master
            WHERE type = 'index'
            ORDER BY tbl_name, name
            "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in index_rows {
        let sql: Option<String> = row.try_get("sql").ok();
        let unique = sql
            .map(|s| s.to_uppercase().contains("UNIQUE"))
            .unwrap_or(false);

        schema_info.indexes.push(IndexInfo {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            name: row.try_get::<String, _>("name").unwrap_or_default(),
            columns: Vec::new(),
            unique,
        });
    }

    Ok(schema_info)
}
