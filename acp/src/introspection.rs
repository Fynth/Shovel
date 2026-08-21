//! Database introspection layer for ACP
//!
//! Provides non-intrusive monitoring of database state including:
//! - Lock status and blocking queries
//! - Active and historical queries
//! - Index usage statistics
//! - Table statistics
//!
//! All introspection queries use a dedicated connection pool with:
//! - Maximum 2 connections (to avoid impacting user queries)
//! - 5-second timeout on all queries
//! - Rate limiting to prevent overload

use std::time::Duration;

use models::{ClickHouseFormData, ConnectionRequest, DatabaseConnection, DatabaseKind};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::time::{Instant, Interval, interval};

/// Configuration for introspection intervals
#[derive(Clone, Debug)]
pub struct IntrospectionConfig {
    /// Interval for checking lock status (lightweight)
    pub lock_status_interval: Duration,
    /// Interval for checking active queries (lightweight)
    pub active_queries_interval: Duration,
    /// Interval for fetching query history (heavy)
    pub query_history_interval: Duration,
    /// Interval for collecting index statistics (heavy)
    pub index_stats_interval: Duration,
    /// Interval for refreshing schema information (heavy)
    pub schema_refresh_interval: Duration,
}

impl Default for IntrospectionConfig {
    fn default() -> Self {
        Self {
            lock_status_interval: Duration::from_secs(5),
            active_queries_interval: Duration::from_secs(5),
            query_history_interval: Duration::from_secs(30),
            index_stats_interval: Duration::from_secs(30),
            schema_refresh_interval: Duration::from_secs(30),
        }
    }
}

/// A dedicated connection pool for introspection queries
///
/// This wrapper ensures:
/// - Max 2 connections to avoid impacting user queries
/// - 5-second timeout on all introspection queries
/// - Separate pool from user connection pool
pub struct IntrospectionPool {
    connection: DatabaseConnection,
    #[allow(dead_code)]
    config: IntrospectionConfig,
}

/// Result of an introspection query
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IntrospectionResult {
    /// Lock status information
    pub locks: Vec<LockInfo>,
    /// Currently active queries
    pub active_queries: Vec<ActiveQueryInfo>,
    /// Query history (slowest queries)
    pub query_history: Vec<QueryHistoryEntry>,
    /// Index usage statistics
    pub index_stats: Vec<IndexStat>,
    /// Table statistics
    pub table_stats: Vec<TableStat>,
    /// Schema information
    pub schema_info: SchemaInfo,
    /// Timestamp when the data was collected
    pub collected_at: Option<u64>,
}

/// Lock information from the database
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LockInfo {
    pub database: String,
    pub relation: Option<String>,
    pub mode: String,
    pub granted: bool,
    pub query: Option<String>,
    pub pid: Option<i64>,
    pub wait_start: Option<String>,
}

/// Active query information
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActiveQueryInfo {
    pub pid: Option<i64>,
    pub database: String,
    pub username: String,
    pub query: String,
    pub state: String,
    pub start_time: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Historical query entry
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub query: String,
    pub calls: i64,
    pub total_time_ms: f64,
    pub mean_time_ms: f64,
    pub rows: i64,
}

/// Index usage statistics
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexStat {
    pub schema: String,
    pub table: String,
    pub index_name: String,
    pub idx_scan: i64,
    pub idx_tup_read: i64,
    pub idx_tup_fetch: i64,
}

/// Table statistics
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TableStat {
    pub schema: String,
    pub table: String,
    pub seq_scan: i64,
    pub seq_tup_read: i64,
    pub idx_scan: i64,
    pub idx_tup_fetch: i64,
    pub n_tup_ins: i64,
    pub n_tup_upd: i64,
    pub n_tup_del: i64,
    pub n_live_tup: i64,
    pub n_dead_tup: i64,
}

/// Schema information
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub tables: Vec<TableInfo>,
    pub indexes: Vec<IndexInfo>,
}

/// Table metadata
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

/// Column metadata
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
}

/// Index metadata
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// Rate limiter for introspection queries
pub struct IntrospectionRateLimiter {
    light_interval: Interval,
    heavy_interval: Interval,
    last_light_run: Option<Instant>,
    last_heavy_run: Option<Instant>,
}

impl IntrospectionRateLimiter {
    pub fn new(config: &IntrospectionConfig) -> Self {
        Self {
            light_interval: interval(config.lock_status_interval),
            heavy_interval: interval(config.query_history_interval),
            last_light_run: None,
            last_heavy_run: None,
        }
    }

    /// Check if a light query (lock status, active queries) can run
    pub fn can_run_light(&self) -> bool {
        self.last_light_run
            .map(|last| last.elapsed() >= Duration::from_secs(5))
            .unwrap_or(true)
    }

    /// Check if a heavy query (query history, index stats, schema) can run
    pub fn can_run_heavy(&self) -> bool {
        self.last_heavy_run
            .map(|last| last.elapsed() >= Duration::from_secs(30))
            .unwrap_or(true)
    }

    /// Mark light query as run
    pub fn mark_light_run(&mut self) {
        self.last_light_run = Some(Instant::now());
    }

    /// Mark heavy query as run
    pub fn mark_heavy_run(&mut self) {
        self.last_heavy_run = Some(Instant::now());
    }

    /// Wait for the next light tick
    pub async fn wait_light(&mut self) {
        self.light_interval.tick().await;
    }

    /// Wait for the next heavy tick
    pub async fn wait_heavy(&mut self) {
        self.heavy_interval.tick().await;
    }
}

impl IntrospectionPool {
    /// Create a new introspection pool from a connection request
    pub async fn new(request: ConnectionRequest) -> Result<Self, String> {
        let connection = Self::create_dedicated_pool(&request).await?;
        Ok(Self {
            connection,
            config: IntrospectionConfig::default(),
        })
    }

    /// Create a new introspection pool with custom config
    pub async fn new_with_config(
        request: ConnectionRequest,
        config: IntrospectionConfig,
    ) -> Result<Self, String> {
        let connection = Self::create_dedicated_pool(&request).await?;
        Ok(Self { connection, config })
    }

    /// Create a dedicated pool with max 2 connections and 5s timeout
    async fn create_dedicated_pool(
        request: &ConnectionRequest,
    ) -> Result<DatabaseConnection, String> {
        match request {
            ConnectionRequest::Sqlite(data) => {
                use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
                let options = SqliteConnectOptions::new()
                    .filename(&data.path)
                    .create_if_missing(false)
                    .busy_timeout(Duration::from_secs(5))
                    .journal_mode(SqliteJournalMode::Wal)
                    .synchronous(SqliteSynchronous::Normal);
                let pool = sqlx::SqlitePool::connect_with(options)
                    .await
                    .map_err(|e| format!("Failed to create introspection pool: {e}"))?;
                Ok(DatabaseConnection::Sqlite(pool))
            }
            ConnectionRequest::Postgres(data) => {
                use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
                let options = PgConnectOptions::new()
                    .host(&data.host)
                    .port(if data.port == 0 { 5432 } else { data.port })
                    .username(&data.username)
                    .password(&data.password)
                    .database(&data.database)
                    .ssl_mode(PgSslMode::Prefer);
                let pool = PgPoolOptions::new()
                    .max_connections(2)
                    .acquire_timeout(Duration::from_secs(5))
                    .connect_with(options)
                    .await
                    .map_err(|e| format!("Failed to create introspection pool: {e}"))?;
                Ok(DatabaseConnection::Postgres(pool))
            }
            ConnectionRequest::MySql(data) => {
                use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
                let options = MySqlConnectOptions::new()
                    .host(&data.host)
                    .port(if data.port == 0 { 3306 } else { data.port })
                    .username(&data.username)
                    .password(&data.password)
                    .database(&data.database)
                    .ssl_mode(MySqlSslMode::Preferred);
                let pool = MySqlPoolOptions::new()
                    .max_connections(2)
                    .acquire_timeout(Duration::from_secs(5))
                    .connect_with(options)
                    .await
                    .map_err(|e| format!("Failed to create introspection pool: {e}"))?;
                Ok(DatabaseConnection::MySql(pool))
            }
            ConnectionRequest::ClickHouse(data) => {
                // ClickHouse uses HTTP, not a pool - just clone the config
                Ok(DatabaseConnection::ClickHouse(data.clone()))
            }
        }
    }

    /// Get the database kind
    pub fn database_kind(&self) -> DatabaseKind {
        match &self.connection {
            DatabaseConnection::Sqlite(_) => DatabaseKind::Sqlite,
            DatabaseConnection::Postgres(_) => DatabaseKind::Postgres,
            DatabaseConnection::MySql(_) => DatabaseKind::MySql,
            DatabaseConnection::ClickHouse(_) => DatabaseKind::ClickHouse,
        }
    }

    /// Run full introspection with rate limiting
    pub async fn introspect(&self) -> IntrospectionResult {
        let mut result = IntrospectionResult::default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        result.collected_at = Some(now);

        // Light queries (can run frequently)
        result.locks = self.collect_locks().await.unwrap_or_default();
        result.active_queries = self.collect_active_queries().await.unwrap_or_default();

        // Heavy queries (rate limited)
        result.query_history = self.collect_query_history().await.unwrap_or_default();
        result.index_stats = self.collect_index_stats().await.unwrap_or_default();
        result.table_stats = self.collect_table_stats().await.unwrap_or_default();
        result.schema_info = self.collect_schema_info().await.unwrap_or_default();

        result
    }

    /// Collect lock information
    async fn collect_locks(&self) -> Result<Vec<LockInfo>, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_locks(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_locks(pool).await,
            DatabaseConnection::ClickHouse(config) => self.collect_clickhouse_locks(config).await,
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_locks(pool).await,
        }
    }

    /// Collect active query information
    async fn collect_active_queries(&self) -> Result<Vec<ActiveQueryInfo>, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_active_queries(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_active_queries(pool).await,
            DatabaseConnection::ClickHouse(config) => {
                self.collect_clickhouse_active_queries(config).await
            }
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_active_queries(pool).await,
        }
    }

    /// Collect query history (slowest queries)
    async fn collect_query_history(&self) -> Result<Vec<QueryHistoryEntry>, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_query_history(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_query_history(pool).await,
            DatabaseConnection::ClickHouse(config) => {
                self.collect_clickhouse_query_history(config).await
            }
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_query_history(pool).await,
        }
    }

    /// Collect index statistics
    async fn collect_index_stats(&self) -> Result<Vec<IndexStat>, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_index_stats(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_index_stats(pool).await,
            DatabaseConnection::ClickHouse(config) => {
                self.collect_clickhouse_index_stats(config).await
            }
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_index_stats(pool).await,
        }
    }

    /// Collect table statistics
    async fn collect_table_stats(&self) -> Result<Vec<TableStat>, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_table_stats(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_table_stats(pool).await,
            DatabaseConnection::ClickHouse(config) => {
                self.collect_clickhouse_table_stats(config).await
            }
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_table_stats(pool).await,
        }
    }

    /// Collect schema information
    async fn collect_schema_info(&self) -> Result<SchemaInfo, String> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => self.collect_pg_schema_info(pool).await,
            DatabaseConnection::MySql(pool) => self.collect_mysql_schema_info(pool).await,
            DatabaseConnection::ClickHouse(config) => {
                self.collect_clickhouse_schema_info(config).await
            }
            DatabaseConnection::Sqlite(pool) => self.collect_sqlite_schema_info(pool).await,
        }
    }
}

// PostgreSQL introspection implementations
impl IntrospectionPool {
    /// Collect lock information from PostgreSQL
    ///
    /// Uses pg_locks joined with pg_stat_activity to show blocking queries
    async fn collect_pg_locks(&self, pool: &sqlx::PgPool) -> Result<Vec<LockInfo>, String> {
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

    /// Collect active queries from PostgreSQL
    async fn collect_pg_active_queries(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Vec<ActiveQueryInfo>, String> {
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

    /// Collect query history from PostgreSQL using pg_stat_statements
    ///
    /// Returns the slowest queries by total time
    async fn collect_pg_query_history(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
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

    /// Collect index statistics from PostgreSQL
    async fn collect_pg_index_stats(&self, pool: &sqlx::PgPool) -> Result<Vec<IndexStat>, String> {
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

    /// Collect table statistics from PostgreSQL
    async fn collect_pg_table_stats(&self, pool: &sqlx::PgPool) -> Result<Vec<TableStat>, String> {
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

    /// Collect schema information from PostgreSQL
    async fn collect_pg_schema_info(&self, pool: &sqlx::PgPool) -> Result<SchemaInfo, String> {
        let mut schema_info = SchemaInfo::default();

        // Get tables
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

            // Get columns for this table
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

        // Get indexes
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
                columns: Vec::new(), // Would need to parse definition
                unique,
            });
        }

        Ok(schema_info)
    }
}

// MySQL introspection implementations
impl IntrospectionPool {
    /// Collect lock information from MySQL
    ///
    /// Uses information_schema.INNODB_LOCK_WAITS and related tables
    async fn collect_mysql_locks(&self, pool: &sqlx::MySqlPool) -> Result<Vec<LockInfo>, String> {
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

    /// Collect active queries from MySQL
    async fn collect_mysql_active_queries(
        &self,
        pool: &sqlx::MySqlPool,
    ) -> Result<Vec<ActiveQueryInfo>, String> {
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

    /// Collect query history from MySQL using performance_schema
    async fn collect_mysql_query_history(
        &self,
        pool: &sqlx::MySqlPool,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
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

    /// Collect index statistics from MySQL
    async fn collect_mysql_index_stats(
        &self,
        pool: &sqlx::MySqlPool,
    ) -> Result<Vec<IndexStat>, String> {
        // MySQL doesn't have direct index usage stats like PostgreSQL
        // We can get index information from information_schema
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

    /// Collect table statistics from MySQL
    async fn collect_mysql_table_stats(
        &self,
        pool: &sqlx::MySqlPool,
    ) -> Result<Vec<TableStat>, String> {
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

    /// Collect schema information from MySQL
    async fn collect_mysql_schema_info(
        &self,
        pool: &sqlx::MySqlPool,
    ) -> Result<SchemaInfo, String> {
        let mut schema_info = SchemaInfo::default();

        // Get tables
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

            // Get columns for this table
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

        // Get indexes
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
}

// ClickHouse introspection implementations
impl IntrospectionPool {
    /// Collect lock information from ClickHouse
    async fn collect_clickhouse_locks(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<Vec<LockInfo>, String> {
        // ClickHouse has different locking model - check system.merges for active merges
        let result = driver_clickhouse::execute_json_query(
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

    /// Collect active queries from ClickHouse
    async fn collect_clickhouse_active_queries(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<Vec<ActiveQueryInfo>, String> {
        let result = driver_clickhouse::execute_json_query(
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

    /// Collect query history from ClickHouse
    async fn collect_clickhouse_query_history(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
        let result = driver_clickhouse::execute_json_query(
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

    /// Collect index statistics from ClickHouse
    async fn collect_clickhouse_index_stats(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<Vec<IndexStat>, String> {
        // ClickHouse has different indexing model - we can get projection info
        let result = driver_clickhouse::execute_json_query(
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

    /// Collect table statistics from ClickHouse
    async fn collect_clickhouse_table_stats(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<Vec<TableStat>, String> {
        let result = driver_clickhouse::execute_json_query(
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

    /// Collect schema information from ClickHouse
    async fn collect_clickhouse_schema_info(
        &self,
        config: &ClickHouseFormData,
    ) -> Result<SchemaInfo, String> {
        let mut schema_info = SchemaInfo::default();

        // Get tables
        let result = driver_clickhouse::execute_json_query(
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

                // Get columns for this table
                let col_result = driver_clickhouse::execute_json_query(
                    config,
                    &format!(
                        r#"
                        SELECT
                            name,
                            type as data_type,
                            default_kind != '' as has_default
                        FROM system.columns
                        WHERE database = '{}' AND table = '{}'
                        ORDER BY position
                        "#,
                        schema, name
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
                            nullable: true, // ClickHouse columns are nullable by default
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
}

// SQLite introspection implementations
impl IntrospectionPool {
    /// Collect lock information from SQLite
    ///
    /// SQLite has different locking - we check for WAL mode and busy status
    async fn collect_sqlite_locks(&self, pool: &sqlx::SqlitePool) -> Result<Vec<LockInfo>, String> {
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

    /// Collect active queries from SQLite
    ///
    /// SQLite doesn't expose running queries directly, but we can check busy status
    async fn collect_sqlite_active_queries(
        &self,
        _pool: &sqlx::SqlitePool,
    ) -> Result<Vec<ActiveQueryInfo>, String> {
        // SQLite doesn't have a pg_stat_activity equivalent
        Ok(Vec::new())
    }

    /// Collect query history from SQLite
    ///
    /// SQLite doesn't have built-in query history
    async fn collect_sqlite_query_history(
        &self,
        _pool: &sqlx::SqlitePool,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
        // SQLite doesn't have pg_stat_statements equivalent
        Ok(Vec::new())
    }

    /// Collect index statistics from SQLite
    async fn collect_sqlite_index_stats(
        &self,
        pool: &sqlx::SqlitePool,
    ) -> Result<Vec<IndexStat>, String> {
        // SQLite doesn't have direct index usage stats
        // We can list indexes from sqlite_master
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

    /// Collect table statistics from SQLite
    async fn collect_sqlite_table_stats(
        &self,
        pool: &sqlx::SqlitePool,
    ) -> Result<Vec<TableStat>, String> {
        // SQLite has some stats via dbstat virtual table if available
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

    /// Collect schema information from SQLite
    async fn collect_sqlite_schema_info(
        &self,
        pool: &sqlx::SqlitePool,
    ) -> Result<SchemaInfo, String> {
        let mut schema_info = SchemaInfo::default();

        // Get tables using PRAGMA table_list
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

            // Get columns for this table
            let column_rows = sqlx::query(&format!("PRAGMA table_info(\"{}\")", name))
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

        // Get indexes
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
}

/// Run EXPLAIN QUERY PLAN for a SQL query on SQLite
pub async fn explain_query_plan_sqlite(
    pool: &sqlx::SqlitePool,
    query: &str,
) -> Result<Vec<String>, String> {
    let explain_query = format!("EXPLAIN QUERY PLAN {}", query);
    let rows = sqlx::query(&explain_query)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to explain query plan: {e}"))?;

    let mut plans = Vec::new();
    for row in rows {
        let detail: String = row.try_get("detail").unwrap_or_default();
        plans.push(detail);
    }
    Ok(plans)
}

/// Run EXPLAIN for a SQL query on PostgreSQL
pub async fn explain_query_plan_postgres(
    pool: &sqlx::PgPool,
    query: &str,
) -> Result<Vec<String>, String> {
    let explain_query = format!("EXPLAIN {}", query);
    let rows = sqlx::query(&explain_query)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to explain query plan: {e}"))?;

    let mut plans = Vec::new();
    for row in rows {
        let plan: String = row.try_get(0).unwrap_or_default();
        plans.push(plan);
    }
    Ok(plans)
}

/// Run EXPLAIN for a SQL query on MySQL
pub async fn explain_query_plan_mysql(
    pool: &sqlx::MySqlPool,
    query: &str,
) -> Result<Vec<String>, String> {
    let explain_query = format!("EXPLAIN {}", query);
    let rows = sqlx::query(&explain_query)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to explain query plan: {e}"))?;

    let mut plans = Vec::new();
    for row in rows {
        // MySQL EXPLAIN returns rows with multiple columns
        let id: Option<i64> = row.try_get("id").ok();
        let select_type: Option<String> = row.try_get("select_type").ok();
        let table: Option<String> = row.try_get("table").ok();
        let plan = format!(
            "id={} select_type={} table={}",
            id.map(|i| i.to_string()).unwrap_or_default(),
            select_type.unwrap_or_default(),
            table.unwrap_or_default()
        );
        plans.push(plan);
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspection_config_default() {
        let config = IntrospectionConfig::default();
        assert_eq!(config.lock_status_interval, Duration::from_secs(5));
        assert_eq!(config.active_queries_interval, Duration::from_secs(5));
        assert_eq!(config.query_history_interval, Duration::from_secs(30));
        assert_eq!(config.index_stats_interval, Duration::from_secs(30));
        assert_eq!(config.schema_refresh_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_introspection_result_default() {
        let result = IntrospectionResult::default();
        assert!(result.locks.is_empty());
        assert!(result.active_queries.is_empty());
        assert!(result.query_history.is_empty());
        assert!(result.index_stats.is_empty());
        assert!(result.table_stats.is_empty());
        assert!(result.schema_info.tables.is_empty());
        assert!(result.schema_info.indexes.is_empty());
    }

    #[tokio::test]
    async fn test_rate_limiter_light() {
        let config = IntrospectionConfig::default();
        let limiter = IntrospectionRateLimiter::new(&config);
        assert!(limiter.can_run_light());
    }

    #[tokio::test]
    async fn test_rate_limiter_heavy() {
        let config = IntrospectionConfig::default();
        let limiter = IntrospectionRateLimiter::new(&config);
        assert!(limiter.can_run_heavy());
    }
}
