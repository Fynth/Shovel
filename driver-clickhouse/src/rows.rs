use models::{ClickHouseJsonResponse, QueryPage};

pub(crate) fn clickhouse_rows_to_page(response: ClickHouseJsonResponse) -> QueryPage {
    QueryPage {
        columns: response
            .meta
            .into_iter()
            .map(|column| column.name)
            .collect(),
        rows: response
            .data
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| clickhouse_json_value_to_string(&value))
                    .collect()
            })
            .collect(),
        editable: None,
        offset: 0,
        page_size: 0,
        has_previous: false,
        has_next: false,
    }
}

pub(crate) fn clickhouse_rows_to_paginated_page(
    mut response: ClickHouseJsonResponse,
    page_size: u32,
    offset: u64,
) -> QueryPage {
    let has_next = response.data.len() > page_size as usize;
    if has_next {
        response.data.truncate(page_size as usize);
    }

    QueryPage {
        columns: response
            .meta
            .into_iter()
            .map(|column| column.name)
            .collect(),
        rows: response
            .data
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| clickhouse_json_value_to_string(&value))
                    .collect()
            })
            .collect(),
        editable: None,
        offset,
        page_size,
        has_previous: offset > 0,
        has_next,
    }
}

fn clickhouse_json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) =>
            serde_json::to_string(value).unwrap_or_else(|_| "<unsupported>".to_string()),
    }
}
