use database::SessionHandle;
use models::{
    DatabaseError,
    DatabaseKind,
    EditableTableContext,
    QueryFilter,
    QueryOutput,
    QuerySort,
    TablePreviewSource,
};

use super::{LOCATOR_COLUMN, build_outer_paginated_query};

pub async fn load_table_preview_page(
    handle: &SessionHandle,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let locator_expr = preview_locator_expr(handle, &source).await?;
    let inner_sql = match locator_expr.as_deref() {
        Some(locator_expr) => format!(
            r#"select {locator_expr} as "{LOCATOR_COLUMN}", * from {}"#,
            source.qualified_name
        ),
        None => format!("select * from {}", source.qualified_name),
    };
    let sql = build_outer_paginated_query(
        inner_sql,
        page_size,
        offset,
        filter.as_ref(),
        sort.as_ref(),
        handle.dialect(),
    );
    match handle.query().execute_sql(&sql).await? {
        QueryOutput::Table(mut page) => {
            if let Some(ctx) = page.editable.as_mut() {
                ctx.source = source;
            } else if locator_expr.is_some() {
                page.editable = Some(EditableTableContext {
                    source,
                    row_locators: vec![String::new(); page.rows.len()],
                });
            }
            Ok(QueryOutput::Table(page))
        }
        other => Ok(other),
    }
}

async fn preview_locator_expr(
    handle: &SessionHandle,
    source: &TablePreviewSource,
) -> Result<Option<String>, DatabaseError> {
    match handle.kind() {
        DatabaseKind::Sqlite => Ok(Some("rowid".into())),
        DatabaseKind::Postgres => Ok(Some("ctid::text".into())),
        DatabaseKind::MySql =>
            handle
                .query()
                .locator_expression(source.schema.clone(), source.table_name.clone())
                .await,
        DatabaseKind::ClickHouse => Ok(None),
    }
}
