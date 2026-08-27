// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use std::sync::Arc;

use async_trait::async_trait;
use models::{
    Capabilities,
    DatabaseConnection,
    DatabaseError,
    DatabaseKind,
    ExecutionPlan,
    ExplorerNode,
    ExplorerNodeKind,
    QueryOutput,
    TableForeignKey,
    TablePreviewSource,
};

use crate::{Dialect, FormatFlavor, quote_ident_double};

#[async_trait]
pub trait QueryExec: Send + Sync {
    /// Run already-built SQL and decode rows. Pagination SQL is built by
    /// `query` via `Dialect`.
    async fn execute_sql(&self, sql: &str) -> Result<QueryOutput, DatabaseError>;
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
    async fn ping(&self) -> Result<(), DatabaseError>;
}

pub trait DriverSession: QueryExec + SchemaExec + Send + Sync {
    fn kind(&self) -> DatabaseKind;
    fn capabilities(&self) -> Capabilities;
    fn dialect(&self) -> Dialect;
    fn as_mutate(&self) -> Option<&dyn MutateExec>;
    fn as_explain(&self) -> Option<&dyn ExplainExec>;
    fn as_introspect(&self) -> Option<&dyn IntrospectExec>;
    fn as_legacy(&self) -> Option<DatabaseConnection> {
        None
    }
}

#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<dyn DriverSession>,
}

impl SessionHandle {
    pub fn wrap(inner: Arc<dyn DriverSession>) -> Self {
        Self { inner }
    }

    pub fn from_legacy(connection: DatabaseConnection) -> Self {
        Self::wrap(Arc::new(LegacyDriver { connection }))
    }

    pub fn legacy(&self) -> Option<DatabaseConnection> {
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

#[derive(Clone)]
struct LegacyDriver {
    connection: DatabaseConnection,
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
        Dialect {
            quote_identifier: quote_ident_double,
            filter_expression: |_, _, _| "1=1".to_string(),
            format_flavor: FormatFlavor::Generic,
        }
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

    fn as_legacy(&self) -> Option<DatabaseConnection> {
        Some(self.connection.clone())
    }
}
