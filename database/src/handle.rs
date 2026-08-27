// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use std::sync::Arc;

use async_trait::async_trait;
use models::{
    Capabilities,
    DatabaseError,
    DatabaseKind,
    ExecutionPlan,
    ExplorerNode,
    ExplorerNodeKind,
    IntrospectionResult,
    QueryOutput,
    TableForeignKey,
    TablePreviewSource,
};

use crate::{Dialect, FormatFlavor, LiveConnection, quote_ident_backtick, quote_ident_double};

#[async_trait]
pub trait QueryExec: Send + Sync {
    /// Run already-built SQL and decode rows. Pagination SQL is built by
    /// `query` via `Dialect`.
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError>;

    /// Optional locator SQL for editable `SELECT` wrappers (MySQL PK `json_array`).
    async fn locator_expression(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }
}

#[async_trait]
pub trait SchemaExec: Send + Sync {
    async fn describe_table(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<QueryOutput, DatabaseError>;
    async fn load_table_columns(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<Vec<String>, DatabaseError>;
    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError>;
    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError>;
    async fn load_object_ddl(
        &self,
        schema: Option<String>,
        object: String,
        kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError>;
}

#[async_trait]
pub trait MutateExec: Send + Sync {
    async fn update_table_cell(
        &self,
        source: TablePreviewSource,
        locator: String,
        column_name: String,
        value: String,
    ) -> Result<(), DatabaseError>;

    async fn insert_table_row(&self, source: TablePreviewSource) -> Result<(), DatabaseError>;

    async fn insert_table_row_with_values(
        &self,
        source: TablePreviewSource,
        column_values: Vec<(String, String)>,
    ) -> Result<(), DatabaseError>;

    async fn delete_table_row(
        &self,
        source: TablePreviewSource,
        locator: String,
    ) -> Result<(), DatabaseError>;

    async fn next_table_primary_key_id(
        &self,
        source: TablePreviewSource,
    ) -> Result<Option<(String, i64)>, DatabaseError>;

    async fn import_csv(
        &self,
        source: TablePreviewSource,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> Result<u64, DatabaseError>;
}

#[async_trait]
pub trait ExplainExec: Send + Sync {
    async fn execute_explain(
        &self,
        sql: &str,
        analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError>;
}

#[async_trait]
pub trait IntrospectExec: Send + Sync {
    async fn introspect(&self) -> IntrospectionResult;
}

pub trait DriverSession: QueryExec + SchemaExec + Send + Sync {
    fn kind(&self) -> DatabaseKind;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> Dialect;
    fn as_mutate(&self) -> Option<&dyn MutateExec>;
    fn as_explain(&self) -> Option<&dyn ExplainExec>;
    fn as_introspect(&self) -> Option<&dyn IntrospectExec>;
    fn as_legacy(&self) -> Option<LiveConnection> {
        None
    }
}

#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<dyn DriverSession>,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("kind", &self.kind())
            .finish()
    }
}

impl SessionHandle {
    pub fn wrap(inner: Arc<dyn DriverSession>) -> Self {
        Self { inner }
    }

    pub fn from_legacy(connection: LiveConnection) -> Self {
        Self::wrap(Arc::new(LegacyDriver { connection }))
    }

    pub fn legacy(&self) -> Option<LiveConnection> {
        self.inner.as_legacy()
    }

    pub fn kind(&self) -> DatabaseKind {
        self.inner.kind()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    pub fn dialect(&self) -> Dialect {
        self.inner.dialect()
    }

    pub fn query(&self) -> &dyn QueryExec {
        self.inner.as_ref()
    }

    pub fn schema(&self) -> &dyn SchemaExec {
        self.inner.as_ref()
    }

    pub fn mutate(&self) -> Option<&dyn MutateExec> {
        self.inner.as_mutate()
    }

    pub fn explain(&self) -> Option<&dyn ExplainExec> {
        self.inner.as_explain()
    }

    pub fn introspect(&self) -> Option<&dyn IntrospectExec> {
        self.inner.as_introspect()
    }
}

const LEGACY_UNSUPPORTED: &str = "legacy driver; use SessionHandle::legacy";

fn legacy_unsupported<T>() -> Result<T, DatabaseError> {
    Err(DatabaseError::Unsupported(LEGACY_UNSUPPORTED.to_string()))
}

/// Maps a [`LiveConnection`] variant (via [`LiveConnection::kind`]) to the
/// dialect `LegacyDriver` reports. Filter expressions stay stubbed: pagination
/// still selects `*_DIALECT` from the live pool, not `handle.dialect()`.
fn dialect_for_live(kind: DatabaseKind) -> Dialect {
    let (quote_identifier, format_flavor): (fn(&str) -> String, FormatFlavor) = match kind {
        DatabaseKind::Postgres => (quote_ident_double, FormatFlavor::Postgres),
        DatabaseKind::Sqlite => (quote_ident_double, FormatFlavor::Generic),
        DatabaseKind::MySql | DatabaseKind::ClickHouse =>
            (quote_ident_backtick, FormatFlavor::Generic),
    };
    Dialect {
        quote_identifier,
        filter_expression: |_, _, _| "1=1".to_string(),
        format_flavor,
    }
}

#[derive(Clone)]
struct LegacyDriver {
    connection: LiveConnection,
}

#[async_trait]
impl QueryExec for LegacyDriver {
    async fn execute_sql(&self, _sql: &str) -> Result<QueryOutput, DatabaseError> {
        legacy_unsupported()
    }
}

#[async_trait]
impl SchemaExec for LegacyDriver {
    async fn describe_table(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        legacy_unsupported()
    }

    async fn load_table_columns(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        legacy_unsupported()
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        legacy_unsupported()
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        legacy_unsupported()
    }

    async fn load_object_ddl(
        &self,
        _schema: Option<String>,
        _object: String,
        _kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        legacy_unsupported()
    }
}

impl DriverSession for LegacyDriver {
    fn kind(&self) -> DatabaseKind {
        self.connection.kind()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::for_kind(self.connection.kind())
    }

    fn dialect(&self) -> Dialect {
        dialect_for_live(self.connection.kind())
    }

    fn as_mutate(&self) -> Option<&dyn MutateExec> {
        None
    }

    fn as_explain(&self) -> Option<&dyn ExplainExec> {
        None
    }

    fn as_introspect(&self) -> Option<&dyn IntrospectExec> {
        None
    }

    fn as_legacy(&self) -> Option<LiveConnection> {
        Some(self.connection.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::dialect_for_live;
    use crate::FormatFlavor;
    use models::DatabaseKind;

    #[test]
    fn from_legacy_postgres_reports_postgres_flavor_sqlite_reports_generic() {
        assert_eq!(
            dialect_for_live(DatabaseKind::Postgres).format_flavor,
            FormatFlavor::Postgres
        );
        assert_eq!(
            dialect_for_live(DatabaseKind::Sqlite).format_flavor,
            FormatFlavor::Generic
        );
    }
}
