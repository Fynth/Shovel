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
    QueryOutput,
    TableForeignKey,
    TablePreviewSource,
};

use crate::Dialect;

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
}

#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<dyn DriverSession>,
}

impl SessionHandle {
    pub fn wrap(inner: Arc<dyn DriverSession>) -> Self {
        Self { inner }
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
