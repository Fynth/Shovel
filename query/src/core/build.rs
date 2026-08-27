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

pub(super) fn sqlite_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let text_expr = format!("cast({} as text)", quote_identifier(column_name));
    match operator {
        QueryFilterOperator::Contains => format!(
            "{text_expr} like {} escape '\\' collate nocase",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::NotContains => format!(
            "{text_expr} not like {} escape '\\' collate nocase",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::Equals => {
            format!("{text_expr} = {} collate nocase", sql_literal(value))
        }
        QueryFilterOperator::NotEquals => {
            format!("{text_expr} != {} collate nocase", sql_literal(value))
        }
        QueryFilterOperator::StartsWith => format!(
            "{text_expr} like {} escape '\\' collate nocase",
            sql_prefix_literal(value)
        ),
        QueryFilterOperator::EndsWith => format!(
            "{text_expr} like {} escape '\\' collate nocase",
            sql_suffix_literal(value)
        ),
        QueryFilterOperator::IsNull => format!("{} is null", quote_identifier(column_name)),
        QueryFilterOperator::IsNotNull => format!("{} is not null", quote_identifier(column_name)),
    }
}

pub(super) fn postgres_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let text_expr = format!("cast({} as text)", quote_identifier(column_name));
    match operator {
        QueryFilterOperator::Contains => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_contains_literal(value)
            )
        }
        QueryFilterOperator::NotContains => {
            format!(
                "{text_expr} not ilike {} escape '\\'",
                sql_contains_literal(value)
            )
        }
        QueryFilterOperator::Equals => {
            format!("lower({text_expr}) = lower({})", sql_literal(value))
        }
        QueryFilterOperator::NotEquals => {
            format!("lower({text_expr}) != lower({})", sql_literal(value))
        }
        QueryFilterOperator::StartsWith => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_prefix_literal(value)
            )
        }
        QueryFilterOperator::EndsWith => {
            format!(
                "{text_expr} ilike {} escape '\\'",
                sql_suffix_literal(value)
            )
        }
        QueryFilterOperator::IsNull => format!("{} is null", quote_identifier(column_name)),
        QueryFilterOperator::IsNotNull => format!("{} is not null", quote_identifier(column_name)),
    }
}

pub(super) fn clickhouse_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let column = quote_identifier_clickhouse(column_name);
    let text_expr = format!("lowerUTF8(toString({column}))");
    let lower_literal = format!("lowerUTF8({})", sql_literal(value));
    match operator {
        QueryFilterOperator::Contains => format!(
            "positionCaseInsensitiveUTF8(toString({column}), {}) > 0",
            sql_literal(value)
        ),
        QueryFilterOperator::NotContains => format!(
            "positionCaseInsensitiveUTF8(toString({column}), {}) = 0",
            sql_literal(value)
        ),
        QueryFilterOperator::Equals => format!("{text_expr} = {lower_literal}"),
        QueryFilterOperator::NotEquals => format!("{text_expr} != {lower_literal}"),
        QueryFilterOperator::StartsWith => {
            format!("startsWith({text_expr}, {lower_literal})")
        }
        QueryFilterOperator::EndsWith => {
            format!("endsWith({text_expr}, {lower_literal})")
        }
        QueryFilterOperator::IsNull => format!("isNull({column})"),
        QueryFilterOperator::IsNotNull => format!("isNotNull({column})"),
    }
}

pub(super) fn mysql_filter_expression(
    column_name: &str,
    operator: QueryFilterOperator,
    value: &str,
) -> String {
    let column = quote_identifier_clickhouse(column_name);
    let text_expr = format!("lower(cast({column} as char))");
    let lower_literal = format!("lower({})", sql_literal(value));
    match operator {
        QueryFilterOperator::Contains => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::NotContains => format!(
            "{text_expr} not like lower({}) escape '\\\\'",
            sql_contains_literal(value)
        ),
        QueryFilterOperator::Equals => format!("{text_expr} = {lower_literal}"),
        QueryFilterOperator::NotEquals => format!("{text_expr} != {lower_literal}"),
        QueryFilterOperator::StartsWith => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_prefix_literal(value)
        ),
        QueryFilterOperator::EndsWith => format!(
            "{text_expr} like lower({}) escape '\\\\'",
            sql_suffix_literal(value)
        ),
        QueryFilterOperator::IsNull => format!("{column} is null"),
        QueryFilterOperator::IsNotNull => format!("{column} is not null"),
    }
}

pub(super) fn sql_literal(value: &str) -> String {
    if value.eq_ignore_ascii_case("null") {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn sql_contains_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    format!("'%{escaped}%'")
}

fn sql_prefix_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    format!("'{escaped}%'")
}

fn sql_suffix_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    format!("'%{escaped}'")
}
