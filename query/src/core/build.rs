use database::Dialect;
use models::{QueryFilter, QueryFilterMode, QueryFilterOperator, QueryFilterRule, QuerySort};

use super::{LOCATOR_COLUMN, editable::EditableSelectPlan};

pub(super) fn build_paginated_query(
    sql: &str,
    page_size: u32,
    offset: u64,
    filter: Option<&QueryFilter>,
    sort: Option<&QuerySort>,
    dialect: Dialect,
) -> String {
    let base_sql = sql.trim().trim_end_matches(';');
    build_outer_paginated_query(
        format!("select * from ({base_sql}) as shovel_page"),
        page_size,
        offset,
        filter,
        sort,
        dialect,
    )
}

pub(super) fn build_editable_paginated_query(
    plan: &EditableSelectPlan,
    page_size: u32,
    offset: u64,
    locator_expr: &str,
    filter: Option<&QueryFilter>,
    sort: Option<&QuerySort>,
    dialect: Dialect,
) -> String {
    let base_query = if plan.tail.is_empty() {
        format!(
            r#"select {locator_expr} as "{LOCATOR_COLUMN}", {} from {}"#,
            plan.select_list, plan.source.qualified_name
        )
    } else {
        format!(
            r#"select {locator_expr} as "{LOCATOR_COLUMN}", {} from {} {}"#,
            plan.select_list, plan.source.qualified_name, plan.tail
        )
    };

    build_outer_paginated_query(base_query, page_size, offset, filter, sort, dialect)
}

pub(super) fn build_outer_paginated_query(
    base_query: String,
    page_size: u32,
    offset: u64,
    filter: Option<&QueryFilter>,
    sort: Option<&QuerySort>,
    dialect: Dialect,
) -> String {
    let limit = page_size as u64 + 1;
    let where_clause = build_filter_clause(filter, dialect.filter_expression);
    let order_by = build_order_by_clause(sort, dialect.quote_identifier);
    format!("{base_query}{where_clause}{order_by} limit {limit} offset {offset}")
}

fn build_filter_clause(
    filter: Option<&QueryFilter>,
    filter_expression_fn: fn(&str, QueryFilterOperator, &str) -> String,
) -> String {
    match filter {
        Some(filter) => {
            let conditions = filter
                .rules
                .iter()
                .filter_map(|rule| build_filter_condition(rule, filter_expression_fn))
                .collect::<Vec<_>>();
            if conditions.is_empty() {
                return String::new();
            }

            let joiner = match filter.mode {
                QueryFilterMode::And => " and ",
                QueryFilterMode::Or => " or ",
            };

            format!(" where ({})", conditions.join(joiner))
        }
        None => String::new(),
    }
}

fn build_filter_condition(
    rule: &QueryFilterRule,
    filter_expression_fn: fn(&str, QueryFilterOperator, &str) -> String,
) -> Option<String> {
    let column_name = rule.column_name.trim();
    if column_name.is_empty() {
        return None;
    }

    if !rule.operator.is_nullary() && rule.value.trim().is_empty() {
        return None;
    }

    Some(filter_expression_fn(
        column_name,
        rule.operator,
        rule.value.trim(),
    ))
}

fn build_order_by_clause(
    sort: Option<&QuerySort>,
    quote_identifier_fn: fn(&str) -> String,
) -> String {
    match sort {
        Some(sort) => format!(
            " order by {} {}",
            quote_identifier_fn(&sort.column_name),
            if sort.descending { "desc" } else { "asc" }
        ),
        None => String::new(),
    }
}

pub(super) fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub(super) fn quote_identifier_clickhouse(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

pub(super) fn sql_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("null") {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}
