use models::{EditableTableContext, QueryPage, TablePreviewSource};
use sqlx::{Column, Row, TypeInfo};

pub(crate) const LOCATOR_COLUMN: &str = "__shovel_locator";

pub(crate) fn sqlite_rows_to_page(rows: Vec<sqlx::sqlite::SqliteRow>) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();

    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        page_size: rows.len() as u32,
        rows,
        editable: None,
        offset: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(crate) fn sqlite_rows_to_paginated_page(
    mut rows: Vec<sqlx::sqlite::SqliteRow>,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let rows: Vec<Vec<String>> = rows
        .into_iter()
        .map(|row| {
            (0..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

pub(crate) fn sqlite_preview_rows_to_paginated_page(
    mut rows: Vec<sqlx::sqlite::SqliteRow>,
    source: TablePreviewSource,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .skip(1)
                .map(|c| c.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    let has_next = rows.len() > page_size as usize;
    if has_next {
        rows.truncate(page_size as usize);
    }
    let row_locators = rows
        .iter()
        .map(|row| {
            row.try_get::<i64, _>(0)
                .map(|v| v.to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let rows = rows
        .into_iter()
        .map(|row| {
            (1..row.columns().len())
                .map(|idx| sqlite_cell_to_string(&row, idx))
                .collect()
        })
        .collect();

    QueryPage {
        columns,
        rows,
        editable: Some(EditableTableContext {
            source,
            row_locators,
        }),
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

fn sqlite_cell_to_string(row: &sqlx::sqlite::SqliteRow, idx: usize) -> String {
    if let Ok(value) = row.try_get::<Option<String>, _>(idx) {
        return value.unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i16>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f32>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<bool>, _>(idx) {
        return value
            .map(|value| value.to_string())
            .unwrap_or_else(|| "NULL".to_string());
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return value
            .map(|bytes| format!("<{} bytes>", bytes.len()))
            .unwrap_or_else(|| "NULL".to_string());
    }

    format!("<unsupported:{}>", row.columns()[idx].type_info().name())
}
