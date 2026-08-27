// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::IntrospectExec;
use models::{
    ActiveQueryInfo,
    ColumnInfo,
    IndexInfo,
    IndexStat,
    IntrospectionResult,
    LockInfo,
    QueryHistoryEntry,
    SchemaInfo,
    TableInfo,
    TableStat,
};
use sqlx::Row;

use crate::session::PostgresSession;

#[async_trait]
impl IntrospectExec for PostgresSession {
    async fn introspect(&self) -> IntrospectionResult {
        let collected_at = Some(unix_secs());
        IntrospectionResult {
            locks: collect_locks(&self.pool).await.unwrap_or_default(),
            active_queries: collect_active_queries(&self.pool).await.unwrap_or_default(),
            query_history: collect_query_history(&self.pool).await.unwrap_or_default(),
            index_stats: collect_index_stats(&self.pool).await.unwrap_or_default(),
            table_stats: collect_table_stats(&self.pool).await.unwrap_or_default(),
            schema_info: collect_schema_info(&self.pool).await.unwrap_or_default(),
            collected_at,
        }
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn collect_locks(pool: &sqlx::PgPool) -> Result<Vec<LockInfo>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                l.relation::regclass::text as relation,
                l.mode,
                l.granted,
                a.query,
                a.pid,
                a.wait_event_start::text as wait_start
            FROM pg_locks l
            LEFT JOIN pg_stat_activity a ON l.pid = a.pid
            WHERE l.granted = false
               OR (l.granted = true AND EXISTS (
                   SELECT 1 FROM pg_locks l2
                   WHERE l2.relation = l.relation
                   AND l2.granted = false
               ))
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect PostgreSQL locks: {e}"))?;

    let mut locks = Vec::new();
    for row in rows {
        locks.push(LockInfo {
            database: "postgres".to_string(),
            relation: row.try_get("relation").ok(),
            mode: row.try_get::<String, _>("mode").unwrap_or_default(),
            granted: row.try_get::<bool, _>("granted").unwrap_or(false),
            query: row.try_get("query").ok(),
            pid: row.try_get::<i32, _>("pid").map(|p| p as i64).ok(),
            wait_start: row.try_get("wait_start").ok(),
        });
    }
    Ok(locks)
}

async fn collect_active_queries(pool: &sqlx::PgPool) -> Result<Vec<ActiveQueryInfo>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                pid,
                datname as database,
                usename as username,
                query,
                state,
                query_start::text as start_time,
                EXTRACT(EPOCH FROM (NOW() - query_start)) * 1000 as duration_ms
            FROM pg_stat_activity
            WHERE state != 'idle'
              AND query IS NOT NULL
              AND query NOT LIKE '%pg_stat_activity%'
            ORDER BY query_start DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect PostgreSQL active queries: {e}"))?;

    let mut queries = Vec::new();
    for row in rows {
        queries.push(ActiveQueryInfo {
            pid: row.try_get::<i32, _>("pid").map(|p| p as i64).ok(),
            database: row.try_get::<String, _>("database").unwrap_or_default(),
            username: row.try_get::<String, _>("username").unwrap_or_default(),
            query: row.try_get::<String, _>("query").unwrap_or_default(),
            state: row.try_get::<String, _>("state").unwrap_or_default(),
            start_time: row.try_get("start_time").ok(),
            duration_ms: row.try_get::<f64, _>("duration_ms").map(|d| d as i64).ok(),
        });
    }
    Ok(queries)
}

async fn collect_query_history(pool: &sqlx::PgPool) -> Result<Vec<QueryHistoryEntry>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                query,
                calls,
                total_exec_time,
                mean_exec_time,
                rows
            FROM pg_stat_statements
            ORDER BY total_exec_time DESC
            LIMIT 50
            "#,
    )
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let mut entries = Vec::new();
            for row in rows {
                entries.push(QueryHistoryEntry {
                    query: row.try_get::<String, _>("query").unwrap_or_default(),
                    calls: row.try_get::<i64, _>("calls").unwrap_or(0),
                    total_time_ms: row.try_get::<f64, _>("total_exec_time").unwrap_or(0.0),
                    mean_time_ms: row.try_get::<f64, _>("mean_exec_time").unwrap_or(0.0),
                    rows: row.try_get::<i64, _>("rows").unwrap_or(0),
                });
            }
            Ok(entries)
        }
        Err(_) => {
            // pg_stat_statements may not be installed
            Ok(Vec::new())
        }
    }
}

async fn collect_index_stats(pool: &sqlx::PgPool) -> Result<Vec<IndexStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                schemaname as schema,
                relname as table,
                indexrelname as index_name,
                idx_scan,
                idx_tup_read,
                idx_tup_fetch
            FROM pg_stat_user_indexes
            ORDER BY idx_scan DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect PostgreSQL index stats: {e}"))?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(IndexStat {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            index_name: row.try_get::<String, _>("index_name").unwrap_or_default(),
            idx_scan: row.try_get::<i64, _>("idx_scan").unwrap_or(0),
            idx_tup_read: row.try_get::<i64, _>("idx_tup_read").unwrap_or(0),
            idx_tup_fetch: row.try_get::<i64, _>("idx_tup_fetch").unwrap_or(0),
        });
    }
    Ok(stats)
}

async fn collect_table_stats(pool: &sqlx::PgPool) -> Result<Vec<TableStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                schemaname as schema,
                relname as table,
                seq_scan,
                seq_tup_read,
                idx_scan,
                idx_tup_fetch,
                n_tup_ins,
                n_tup_upd,
                n_tup_del,
                n_live_tup,
                n_dead_tup
            FROM pg_stat_user_tables
            ORDER BY n_live_tup DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect PostgreSQL table stats: {e}"))?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(TableStat {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            seq_scan: row.try_get::<i64, _>("seq_scan").unwrap_or(0),
            seq_tup_read: row.try_get::<i64, _>("seq_tup_read").unwrap_or(0),
            idx_scan: row.try_get::<i64, _>("idx_scan").unwrap_or(0),
            idx_tup_fetch: row.try_get::<i64, _>("idx_tup_fetch").unwrap_or(0),
            n_tup_ins: row.try_get::<i64, _>("n_tup_ins").unwrap_or(0),
            n_tup_upd: row.try_get::<i64, _>("n_tup_upd").unwrap_or(0),
            n_tup_del: row.try_get::<i64, _>("n_tup_del").unwrap_or(0),
            n_live_tup: row.try_get::<i64, _>("n_live_tup").unwrap_or(0),
            n_dead_tup: row.try_get::<i64, _>("n_dead_tup").unwrap_or(0),
        });
    }
    Ok(stats)
}

async fn collect_schema_info(pool: &sqlx::PgPool) -> Result<SchemaInfo, String> {
    let mut schema_info = SchemaInfo::default();

    let table_rows = sqlx::query(
        r#"
            SELECT
                table_schema as schema,
                table_name as name
            FROM information_schema.tables
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY table_schema, table_name
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect PostgreSQL schema info: {e}"))?;

    for row in table_rows {
        let schema: String = row.try_get("schema").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();

        let column_rows = sqlx::query(
            r#"
                SELECT
                    column_name as name,
                    data_type,
                    is_nullable = 'YES' as nullable,
                    column_default as default_value
                FROM information_schema.columns
                WHERE table_schema = $1 AND table_name = $2
                ORDER BY ordinal_position
                "#,
        )
        .bind(&schema)
        .bind(&name)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut columns = Vec::new();
        for col_row in column_rows {
            columns.push(ColumnInfo {
                name: col_row.try_get::<String, _>("name").unwrap_or_default(),
                data_type: col_row
                    .try_get::<String, _>("data_type")
                    .unwrap_or_default(),
                nullable: col_row.try_get::<bool, _>("nullable").unwrap_or(true),
                default: col_row.try_get("default_value").ok(),
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
                schemaname as schema,
                tablename as table,
                indexname as name,
                indexdef as definition
            FROM pg_indexes
            WHERE schemaname NOT IN ('pg_catalog', 'information_schema')
            ORDER BY schemaname, tablename, indexname
            "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in index_rows {
        let definition: String = row.try_get::<String, _>("definition").unwrap_or_default();
        let unique = definition.to_uppercase().contains("UNIQUE");

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
