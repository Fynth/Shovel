use serde::{Deserialize, Serialize};

/// Result of an introspection query.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IntrospectionResult {
    /// Lock status information.
    pub locks: Vec<LockInfo>,
    /// Currently active queries.
    pub active_queries: Vec<ActiveQueryInfo>,
    /// Query history (slowest queries).
    pub query_history: Vec<QueryHistoryEntry>,
    /// Index usage statistics.
    pub index_stats: Vec<IndexStat>,
    /// Table statistics.
    pub table_stats: Vec<TableStat>,
    /// Schema information.
    pub schema_info: SchemaInfo,
    /// Timestamp when the data was collected.
    pub collected_at: Option<u64>,
}

/// Lock information from the database.
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

/// Active query information.
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

/// Historical query entry.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub query: String,
    pub calls: i64,
    pub total_time_ms: f64,
    pub mean_time_ms: f64,
    pub rows: i64,
}

/// Index usage statistics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexStat {
    pub schema: String,
    pub table: String,
    pub index_name: String,
    pub idx_scan: i64,
    pub idx_tup_read: i64,
    pub idx_tup_fetch: i64,
}

/// Table statistics.
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

/// Schema information.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub tables: Vec<TableInfo>,
    pub indexes: Vec<IndexInfo>,
}

/// Table metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

/// Column metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
}

/// Index metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[cfg(test)]
mod tests {
    use super::IntrospectionResult;

    #[test]
    fn introspection_result_default_is_empty() {
        let result = IntrospectionResult::default();
        assert!(result.locks.is_empty());
        assert!(result.active_queries.is_empty());
        assert!(result.query_history.is_empty());
        assert!(result.index_stats.is_empty());
        assert!(result.table_stats.is_empty());
        assert!(result.schema_info.tables.is_empty());
        assert!(result.schema_info.indexes.is_empty());
    }
}
