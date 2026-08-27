use database::SessionHandle;
use models::{DatabaseError, TablePreviewSource};

fn require_mutate(handle: &SessionHandle) -> Result<&dyn database::MutateExec, DatabaseError> {
    handle.mutate().ok_or_else(|| {
        DatabaseError::Unsupported("row editing is not supported for this session".into())
    })
}

pub async fn update_table_cell(
    handle: &SessionHandle,
    source: TablePreviewSource,
    locator: String,
    column_name: String,
    value: String,
) -> Result<(), DatabaseError> {
    require_mutate(handle)?
        .update_table_cell(source, locator, column_name, value)
        .await
}

pub async fn insert_table_row(
    handle: &SessionHandle,
    source: TablePreviewSource,
) -> Result<(), DatabaseError> {
    require_mutate(handle)?.insert_table_row(source).await
}

pub async fn insert_table_row_with_values(
    handle: &SessionHandle,
    source: TablePreviewSource,
    column_values: Vec<(String, String)>,
) -> Result<(), DatabaseError> {
    require_mutate(handle)?
        .insert_table_row_with_values(source, column_values)
        .await
}

pub async fn next_table_primary_key_id(
    handle: &SessionHandle,
    source: TablePreviewSource,
) -> Result<Option<(String, i64)>, DatabaseError> {
    require_mutate(handle)?
        .next_table_primary_key_id(source)
        .await
}

pub async fn delete_table_row(
    handle: &SessionHandle,
    source: TablePreviewSource,
    locator: String,
) -> Result<(), DatabaseError> {
    require_mutate(handle)?
        .delete_table_row(source, locator)
        .await
}
