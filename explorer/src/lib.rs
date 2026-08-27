use database::SessionHandle;
use models::{DatabaseError, ExplorerNode, ExplorerNodeKind, QueryOutput, TableForeignKey};

pub async fn describe_table(
    handle: &SessionHandle,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    handle.schema().describe_table(schema, table).await
}

pub async fn load_table_columns(
    handle: &SessionHandle,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    handle.schema().load_table_columns(schema, table).await
}

pub async fn load_connection_tree(
    handle: &SessionHandle,
) -> Result<Vec<ExplorerNode>, DatabaseError> {
    handle.schema().load_connection_tree().await
}

/// Загружает внешние ключи подключения для ER-диаграммы.
pub async fn load_foreign_keys(
    handle: &SessionHandle,
) -> Result<Vec<TableForeignKey>, DatabaseError> {
    handle.schema().load_foreign_keys().await
}

/// Возвращает DDL объекта (таблицы/представления) или `None`, если объект
/// не найден.
pub async fn load_object_ddl(
    handle: &SessionHandle,
    schema: Option<String>,
    object: String,
    kind: ExplorerNodeKind,
) -> Result<Option<String>, DatabaseError> {
    handle.schema().load_object_ddl(schema, object, kind).await
}
