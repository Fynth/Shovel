//! Database introspection layer for ACP
//!
//! Provides non-intrusive monitoring of database state including:
//! - Lock status and blocking queries
//! - Active and historical queries
//! - Index usage statistics
//! - Table statistics
//!
//! All introspection queries use a dedicated connection obtained via
//! [`connection::connect_to_db_with_tunnel_key`]. The handle is **not**
//! registered in the session registry, so dropping this pool drops the
//! dedicated connection. Any SSH tunnel is registered under
//! [`introspection_ssh_tunnel_key`] (not the UI session identity key) and
//! released when the pool is dropped.

use std::time::Duration;

use database::SessionHandle;
use models::{ConnectionRequest, DatabaseKind};
use tokio::time::{Instant, Interval, interval};

pub use models::{
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

/// Suffix appended to [`ConnectionRequest::identity_key`] so an ephemeral
/// ACP introspection SSH tunnel cannot clobber the UI session tunnel.
const INTROSPECTION_TUNNEL_KEY_SUFFIX: &str = "::acp-introspect";

/// SSH tunnel registry key for a dedicated ACP introspection connection.
///
/// Distinct from [`ConnectionRequest::identity_key`] so registering the
/// introspection tunnel cannot shut down the UI session tunnel that shares
/// the same request.
pub fn introspection_ssh_tunnel_key(identity_key: &str) -> String {
    format!("{identity_key}{INTROSPECTION_TUNNEL_KEY_SUFFIX}")
}

/// A dedicated connection for introspection queries.
///
/// Created through [`connection::connect_to_db_with_tunnel_key`] and **not**
/// registered as a UI session. Dropping the pool drops the handle, then
/// releases any SSH tunnel registered under [`introspection_ssh_tunnel_key`].
pub struct IntrospectionPool {
    handle: Option<SessionHandle>,
    ssh_tunnel_key: String,
    #[allow(dead_code)]
    config: IntrospectionConfig,
}

impl Drop for IntrospectionPool {
    fn drop(&mut self) {
        drop(self.handle.take());
        connection::release_ssh_tunnel(&self.ssh_tunnel_key);
    }
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
    /// Create a dedicated introspection pool from a connection request.
    ///
    /// Calls [`connection::connect_to_db_with_tunnel_key`] with
    /// [`introspection_ssh_tunnel_key`] and does **not**
    /// [`connection::register_session`].
    pub async fn from_request(request: ConnectionRequest) -> Result<Self, String> {
        Self::from_request_with_config(request, IntrospectionConfig::default()).await
    }

    /// Create a new introspection pool from a connection request
    pub async fn new(request: ConnectionRequest) -> Result<Self, String> {
        Self::from_request(request).await
    }

    /// Create a new introspection pool with custom config
    pub async fn new_with_config(
        request: ConnectionRequest,
        config: IntrospectionConfig,
    ) -> Result<Self, String> {
        Self::from_request_with_config(request, config).await
    }

    async fn from_request_with_config(
        request: ConnectionRequest,
        config: IntrospectionConfig,
    ) -> Result<Self, String> {
        let ssh_tunnel_key = introspection_ssh_tunnel_key(&request.identity_key());
        let handle = connection::connect_to_db_with_tunnel_key(request, &ssh_tunnel_key)
            .await
            .map_err(|e| format!("Failed to create introspection pool: {e}"))?;
        Ok(Self {
            handle: Some(handle),
            ssh_tunnel_key,
            config,
        })
    }

    fn handle(&self) -> &SessionHandle {
        self.handle
            .as_ref()
            .expect("IntrospectionPool used after drop")
    }

    /// Get the database kind
    pub fn database_kind(&self) -> DatabaseKind {
        self.handle().kind()
    }

    /// Run full introspection with rate limiting
    pub async fn introspect(&self) -> IntrospectionResult {
        match self.handle().introspect() {
            Some(exec) => exec.introspect().await,
            None => IntrospectionResult {
                collected_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                ),
                ..Default::default()
            },
        }
    }
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

    #[test]
    fn introspection_ssh_tunnel_key_differs_from_session_identity() {
        let request = ConnectionRequest::Postgres(models::PostgresFormData {
            host: "db.example.com".to_string(),
            port: 5432,
            username: "alice".to_string(),
            password: "secret".to_string(),
            database: "app".to_string(),
            ssl_mode: "prefer".to_string(),
            ssh_tunnel: Some(models::SshTunnelConfig {
                host: "bastion.example.com".to_string(),
                port: 22,
                username: "ops".to_string(),
                private_key_path: String::new(),
            }),
        });
        let identity = request.identity_key();
        let ephemeral = introspection_ssh_tunnel_key(&identity);
        assert_ne!(ephemeral, identity);
        assert_eq!(ephemeral, format!("{identity}::acp-introspect"));
    }

    #[tokio::test]
    async fn introspection_pool_stores_ephemeral_ssh_key() {
        let request = ConnectionRequest::Sqlite(models::SqliteFormData {
            path: ":memory:".to_string(),
        });
        let identity = request.identity_key();
        let pool = IntrospectionPool::from_request(request)
            .await
            .expect("sqlite :memory: connect");
        assert_ne!(pool.ssh_tunnel_key, identity);
        assert_eq!(pool.ssh_tunnel_key, introspection_ssh_tunnel_key(&identity));
        drop(pool);
    }
}
