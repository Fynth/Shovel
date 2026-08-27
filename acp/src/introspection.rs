//! Database introspection layer for ACP
//!
//! Provides non-intrusive monitoring of database state including:
//! - Lock status and blocking queries
//! - Active and historical queries
//! - Index usage statistics
//! - Table statistics
//!
//! All introspection queries use a dedicated connection obtained via
//! [`connection::connect_to_db`]. The handle is **not** registered in the
//! session registry, so dropping this pool drops the dedicated connection.

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

/// A dedicated connection for introspection queries.
///
/// Created through [`connection::connect_to_db`] and **not** registered as a
/// UI session. Dropping the pool drops the handle.
pub struct IntrospectionPool {
    handle: SessionHandle,
    #[allow(dead_code)]
    config: IntrospectionConfig,
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
    /// Calls [`connection::connect_to_db`] and does **not**
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
        let handle = connection::connect_to_db(request)
            .await
            .map_err(|e| format!("Failed to create introspection pool: {e}"))?;
        Ok(Self { handle, config })
    }

    /// Get the database kind
    pub fn database_kind(&self) -> DatabaseKind {
        self.handle.kind()
    }

    /// Run full introspection with rate limiting
    pub async fn introspect(&self) -> IntrospectionResult {
        match self.handle.introspect() {
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
}
