// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::IntrospectExec;
use models::{
    ActiveQueryInfo,
    ClickHouseFormData,
    ColumnInfo,
    IndexStat,
    IntrospectionResult,
    LockInfo,
    QueryHistoryEntry,
    SchemaInfo,
    TableInfo,
    TableStat,
};

use crate::session::ClickHouseSession;

#[async_trait]
impl IntrospectExec for ClickHouseSession {
    async fn introspect(&self) -> IntrospectionResult {
        let collected_at = Some(unix_secs());
        IntrospectionResult {
            locks: collect_locks(&self.config).await.unwrap_or_default(),
            active_queries: collect_active_queries(&self.config)
                .await
                .unwrap_or_default(),
            query_history: collect_query_history(&self.config)
                .await
                .unwrap_or_default(),
            index_stats: collect_index_stats(&self.config).await.unwrap_or_default(),
            table_stats: collect_table_stats(&self.config).await.unwrap_or_default(),
            schema_info: collect_schema_info(&self.config).await.unwrap_or_default(),
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

async fn collect_locks(config: &ClickHouseFormData) -> Result<Vec<LockInfo>, String> {
    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                database,
                table,
                'MERGE' as mode,
                true as granted,
                elapsed as duration_seconds
            FROM system.merges
            LIMIT 100
            "#,
    )
    .await;

    match result {
        Ok(response) => {
            let mut locks = Vec::new();
            for row in response.data {
                locks.push(LockInfo {
                    database: row
                        .first()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    relation: row.get(1).and_then(|v| v.as_str().map(|s| s.to_string())),
                    mode: row
                        .get(2)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    granted: row.get(3).and_then(|v| v.as_bool()).unwrap_or(true),
                    query: None,
                    pid: None,
                    wait_start: row.get(4).and_then(|v| v.as_f64().map(|s| s.to_string())),
                });
            }
            Ok(locks)
        }
        Err(_) => Ok(Vec::new()),
    }
}

async fn collect_active_queries(
    config: &ClickHouseFormData,
) -> Result<Vec<ActiveQueryInfo>, String> {
    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                query_id as pid,
                user as username,
                query,
                elapsed as duration_seconds
            FROM system.processes
            WHERE query NOT LIKE '%system.processes%'
            LIMIT 100
            "#,
    )
    .await;

    match result {
        Ok(response) => {
            let mut queries = Vec::new();
            for row in response.data {
                queries.push(ActiveQueryInfo {
                    pid: row
                        .first()
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok()),
                    database: config.database.clone(),
                    username: row
                        .get(1)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    query: row
                        .get(2)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    state: "running".to_string(),
                    start_time: None,
                    duration_ms: row
                        .get(3)
                        .and_then(|v| v.as_f64().map(|s| (s * 1000.0) as i64)),
                });
            }
            Ok(queries)
        }
        Err(_) => Ok(Vec::new()),
    }
}

async fn collect_query_history(
    config: &ClickHouseFormData,
) -> Result<Vec<QueryHistoryEntry>, String> {
    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                query,
                count() as calls,
                sum(query_duration_ms) as total_time_ms,
                avg(query_duration_ms) as mean_time_ms,
                sum(read_rows) as rows
            FROM system.query_log
            WHERE event_date >= today() - 1
            GROUP BY query
            ORDER BY total_time_ms DESC
            LIMIT 50
            "#,
    )
    .await;

    match result {
        Ok(response) => {
            let mut entries = Vec::new();
            for row in response.data {
                entries.push(QueryHistoryEntry {
                    query: row
                        .first()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    calls: row.get(1).and_then(|v| v.as_i64()).unwrap_or(0),
                    total_time_ms: row.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    mean_time_ms: row.get(3).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    rows: row.get(4).and_then(|v| v.as_i64()).unwrap_or(0),
                });
            }
            Ok(entries)
        }
        Err(_) => Ok(Vec::new()),
    }
}

async fn collect_index_stats(config: &ClickHouseFormData) -> Result<Vec<IndexStat>, String> {
    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                database,
                table,
                name as index_name,
                type
            FROM system.data_skipping_indices
            LIMIT 100
            "#,
    )
    .await;

    match result {
        Ok(response) => {
            let mut stats = Vec::new();
            for row in response.data {
                stats.push(IndexStat {
                    schema: row
                        .first()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    table: row
                        .get(1)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    index_name: row
                        .get(2)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    idx_scan: 0,
                    idx_tup_read: 0,
                    idx_tup_fetch: 0,
                });
            }
            Ok(stats)
        }
        Err(_) => Ok(Vec::new()),
    }
}

async fn collect_table_stats(config: &ClickHouseFormData) -> Result<Vec<TableStat>, String> {
    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                database as schema,
                name as table,
                total_rows as n_live_tup,
                total_bytes as total_bytes
            FROM system.tables
            WHERE database NOT IN ('system', 'information_schema')
            ORDER BY total_rows DESC
            LIMIT 100
            "#,
    )
    .await;

    match result {
        Ok(response) => {
            let mut stats = Vec::new();
            for row in response.data {
                stats.push(TableStat {
                    schema: row
                        .first()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    table: row
                        .get(1)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                    seq_scan: 0,
                    seq_tup_read: 0,
                    idx_scan: 0,
                    idx_tup_fetch: 0,
                    n_tup_ins: 0,
                    n_tup_upd: 0,
                    n_tup_del: 0,
                    n_live_tup: row.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                    n_dead_tup: 0,
                });
            }
            Ok(stats)
        }
        Err(_) => Ok(Vec::new()),
    }
}

async fn collect_schema_info(config: &ClickHouseFormData) -> Result<SchemaInfo, String> {
    let mut schema_info = SchemaInfo::default();

    let result = crate::execute_json_query(
        config,
        r#"
            SELECT
                database as schema,
                name
            FROM system.tables
            WHERE database NOT IN ('system', 'information_schema')
            ORDER BY database, name
            "#,
    )
    .await;

    if let Ok(response) = result {
        for row in response.data {
            let schema = row
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = row
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let col_result = crate::execute_json_query(
                config,
                &format!(
                    r#"
                        SELECT
                            name,
                            type as data_type,
                            default_kind != '' as has_default
                        FROM system.columns
                        WHERE database = '{schema}' AND table = '{name}'
                        ORDER BY position
                        "#
                ),
            )
            .await;

            let mut columns = Vec::new();
            if let Ok(col_response) = col_result {
                for col_row in col_response.data {
                    columns.push(ColumnInfo {
                        name: col_row
                            .first()
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default(),
                        data_type: col_row
                            .get(1)
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default(),
                        nullable: true,
                        default: None,
                    });
                }
            }

            schema_info.tables.push(TableInfo {
                schema,
                name,
                columns,
            });
        }
    }

    Ok(schema_info)
}
