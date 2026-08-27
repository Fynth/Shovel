use async_trait::async_trait;
use models::{
    Capabilities,
    DatabaseError,
    DatabaseKind,
    ExplorerNode,
    ExplorerNodeKind,
    QueryOutput,
    QueryPage,
    TableForeignKey,
};

use crate::{
    Dialect,
    DriverSession,
    ExplainExec,
    FormatFlavor,
    IntrospectExec,
    MutateExec,
    QueryExec,
    SchemaExec,
    quote_ident_double,
};

pub struct FakeDriver {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Default for FakeDriver {
    fn default() -> Self {
        Self {
            columns: vec!["id".into(), "name".into()],
            rows: vec![vec!["1".into(), "alpha".into()]],
        }
    }
}

impl FakeDriver {
    fn items_page(&self) -> QueryPage {
        QueryPage {
            columns: self.columns.clone(),
            rows: self.rows.clone(),
            editable: None,
            offset: 0,
            page_size: 10,
            has_previous: false,
            has_next: false,
        }
    }
}

#[async_trait]
impl QueryExec for FakeDriver {
    async fn execute_sql(&self, _sql: &str) -> Result<QueryOutput, DatabaseError> {
        Ok(QueryOutput::Table(self.items_page()))
    }
}

#[async_trait]
impl SchemaExec for FakeDriver {
    async fn describe_table(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        Ok(QueryOutput::Table(self.items_page()))
    }

    async fn load_table_columns(
        &self,
        _schema: Option<String>,
        _table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        Ok(self.columns.clone())
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        Ok(vec![ExplorerNode {
            name: "items".into(),
            kind: ExplorerNodeKind::Table,
            schema: None,
            qualified_name: "items".into(),
            row_count: None,
            children: Vec::new(),
        }])
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn load_object_ddl(
        &self,
        _schema: Option<String>,
        _object: String,
        _kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }
}

impl DriverSession for FakeDriver {
    fn kind(&self) -> DatabaseKind {
        DatabaseKind::Sqlite
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            row_editing: false,
            explain: false,
            transactions: false,
            schemas: false,
            import_csv: false,
            ssh_tunnel: false,
        }
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
}

#[cfg(test)]
mod tests {
    use super::FakeDriver;
    use crate::SessionHandle;
    use std::sync::Arc;

    #[tokio::test]
    async fn fake_execute_sql_returns_rows() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let out = handle.query().execute_sql("select 1").await.unwrap();
        match out {
            models::QueryOutput::Table(page) => {
                assert_eq!(page.columns, vec!["id", "name"]);
                assert!(!page.rows.is_empty());
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn fake_has_no_mutate_when_row_editing_false() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        assert!(!handle.capabilities().row_editing);
        assert!(handle.mutate().is_none());
    }

    #[tokio::test]
    async fn fake_schema_tree_has_items() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let tree = handle.schema().load_connection_tree().await.unwrap();
        assert!(
            tree.iter().any(|node| node.name == "items"),
            "expected items node, got {tree:?}"
        );
    }

    #[test]
    fn fake_has_no_introspect() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        assert!(handle.introspect().is_none());
    }
}
