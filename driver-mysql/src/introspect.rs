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

use crate::session::MysqlSession;

#[async_trait]
impl IntrospectExec for MysqlSession {
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

async fn collect_locks(pool: &sqlx::MySqlPool) -> Result<Vec<LockInfo>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                r.object_schema as database_name,
                r.object_name as relation,
                'WAITING' as mode,
                false as granted,
                w.thread_id as pid,
                NULL as query
            FROM performance_schema.data_lock_waits w
            JOIN performance_schema.data_locks r ON w.requesting_engine_transaction_id = r.engine_transaction_id
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let mut locks = Vec::new();
            for row in rows {
                locks.push(LockInfo {
                    database: row
                        .try_get::<String, _>("database_name")
                        .unwrap_or_default(),
                    relation: row.try_get("relation").ok(),
                    mode: row.try_get::<String, _>("mode").unwrap_or_default(),
                    granted: row
                        .try_get::<i8, _>("granted")
                        .map(|g| g != 0)
                        .unwrap_or(false),
                    query: row.try_get("query").ok(),
                    pid: row.try_get::<i64, _>("pid").ok(),
                    wait_start: None,
                });
            }
            Ok(locks)
        }
        Err(_) => {
            // performance_schema may not be available
            Ok(Vec::new())
        }
    }
}

async fn collect_active_queries(pool: &sqlx::MySqlPool) -> Result<Vec<ActiveQueryInfo>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                id as pid,
                db as database,
                user as username,
                info as query,
                command as state,
                time as duration_seconds
            FROM information_schema.processlist
            WHERE command != 'Sleep'
              AND info IS NOT NULL
              AND info NOT LIKE '%information_schema.processlist%'
            ORDER BY time DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect MySQL active queries: {e}"))?;

    let mut queries = Vec::new();
    for row in rows {
        queries.push(ActiveQueryInfo {
            pid: row.try_get::<i64, _>("pid").ok(),
            database: row.try_get::<String, _>("database").unwrap_or_default(),
            username: row.try_get::<String, _>("username").unwrap_or_default(),
            query: row.try_get::<String, _>("query").unwrap_or_default(),
            state: row.try_get::<String, _>("state").unwrap_or_default(),
            start_time: None,
            duration_ms: row
                .try_get::<i64, _>("duration_seconds")
                .map(|s| s * 1000)
                .ok(),
        });
    }
    Ok(queries)
}

async fn collect_query_history(pool: &sqlx::MySqlPool) -> Result<Vec<QueryHistoryEntry>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                digest_text as query,
                count_star as calls,
                sum_timer_wait / 1000000000 as total_time_ms,
                avg_timer_wait / 1000000000 as mean_time_ms,
                sum_rows_sent as rows
            FROM performance_schema.events_statements_summary_by_digest
            ORDER BY sum_timer_wait DESC
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
                    total_time_ms: row.try_get::<f64, _>("total_time_ms").unwrap_or(0.0),
                    mean_time_ms: row.try_get::<f64, _>("mean_time_ms").unwrap_or(0.0),
                    rows: row.try_get::<i64, _>("rows").unwrap_or(0),
                });
            }
            Ok(entries)
        }
        Err(_) => {
            // performance_schema may not be available
            Ok(Vec::new())
        }
    }
}

async fn collect_index_stats(pool: &sqlx::MySqlPool) -> Result<Vec<IndexStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                table_schema as schema,
                table_name as table,
                index_name,
                cardinality
            FROM information_schema.statistics
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY cardinality DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect MySQL index stats: {e}"))?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(IndexStat {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            index_name: row.try_get::<String, _>("index_name").unwrap_or_default(),
            idx_scan: row.try_get::<i64, _>("cardinality").unwrap_or(0),
            idx_tup_read: 0,
            idx_tup_fetch: 0,
        });
    }
    Ok(stats)
}

async fn collect_table_stats(pool: &sqlx::MySqlPool) -> Result<Vec<TableStat>, String> {
    let rows = sqlx::query(
        r#"
            SELECT
                table_schema as schema,
                table_name as table,
                table_rows as n_live_tup,
                data_length + index_length as total_bytes
            FROM information_schema.tables
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_rows DESC
            LIMIT 100
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect MySQL table stats: {e}"))?;

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
            n_live_tup: row.try_get::<i64, _>("n_live_tup").unwrap_or(0),
            n_dead_tup: 0,
        });
    }
    Ok(stats)
}

async fn collect_schema_info(pool: &sqlx::MySqlPool) -> Result<SchemaInfo, String> {
    let mut schema_info = SchemaInfo::default();

    let table_rows = sqlx::query(
        r#"
            SELECT
                table_schema as schema,
                table_name as name
            FROM information_schema.tables
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to collect MySQL schema info: {e}"))?;

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
                WHERE table_schema = ? AND table_name = ?
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
                nullable: col_row
                    .try_get::<i8, _>("nullable")
                    .map(|n| n != 0)
                    .unwrap_or(true),
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
                table_schema as schema,
                table_name as table,
                index_name as name,
                non_unique = 0 as is_unique
            FROM information_schema.statistics
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            GROUP BY table_schema, table_name, index_name, non_unique
            ORDER BY table_schema, table_name, index_name
            "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in index_rows {
        schema_info.indexes.push(IndexInfo {
            schema: row.try_get::<String, _>("schema").unwrap_or_default(),
            table: row.try_get::<String, _>("table").unwrap_or_default(),
            name: row.try_get::<String, _>("name").unwrap_or_default(),
            columns: Vec::new(),
            unique: row
                .try_get::<i64, _>("is_unique")
                .map(|u| u != 0)
                .unwrap_or(false),
        });
    }

    Ok(schema_info)
}
