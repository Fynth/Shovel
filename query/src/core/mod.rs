mod build;
mod ddl;
mod editable;
mod execution_plan;
pub mod multi;
mod mutations;
mod preview;
pub mod splitter;

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

pub use ddl::{create_table, drop_table, duplicate_table, rename_table, truncate_table};
pub use execution_plan::execute_explain;
pub use mutations::{
    delete_table_row,
    insert_table_row,
    insert_table_row_with_values,
    next_table_primary_key_id,
    update_table_cell,
};
pub use preview::load_table_preview_page;

use self::{
    build::{
        build_editable_paginated_query,
        build_outer_paginated_query,
        build_paginated_query,
        quote_identifier,
        quote_identifier_clickhouse,
        sql_literal,
    },
    editable::editable_select_plan,
};

const LOCATOR_COLUMN: &str = "__shovel_locator";

pub async fn execute_query(
    handle: &SessionHandle,
    sql: String,
) -> Result<QueryOutput, DatabaseError> {
    execute_query_page(handle, sql, 100, 0, None, None).await
}

pub fn is_read_only_sql(sql: &str) -> bool {
    let keywords = statement_leading_keywords(sql);
    !keywords.is_empty()
        && keywords.iter().all(|keyword| {
            matches!(
                keyword.as_str(),
                "select" | "with" | "show" | "describe" | "explain" | "pragma"
            )
        })
}

pub fn preview_source_for_sql(sql: &str) -> Option<TablePreviewSource> {
    editable_select_plan(sql).map(|plan| plan.source)
}

pub async fn execute_query_page(
    handle: &SessionHandle,
    sql: String,
    page_size: u32,
    offset: u64,
    filter: Option<QueryFilter>,
    sort: Option<QuerySort>,
) -> Result<QueryOutput, DatabaseError> {
    let mysql_locator = mysql_locator_expr(handle, &sql).await?;
    let (built_sql, used_locator) = sql_for_query_exec(
        handle,
        &sql,
        page_size,
        offset,
        filter.as_ref(),
        sort.as_ref(),
        mysql_locator.as_deref(),
    );
    match handle.query().execute_sql(&built_sql).await {
        Ok(out) => Ok(with_editable_source(out, &sql, used_locator)),
        Err(err) => Err(err),
    }
}

fn sql_for_query_exec(
    handle: &SessionHandle,
    sql: &str,
    page_size: u32,
    offset: u64,
    filter: Option<&QueryFilter>,
    sort: Option<&QuerySort>,
    locator_override: Option<&str>,
) -> (String, bool) {
    if !is_paginated_query(sql) {
        return (sql.to_string(), false);
    }

    let dialect = handle.dialect();
    let locator_expr = locator_override.or(match handle.kind() {
        DatabaseKind::Sqlite => Some("rowid"),
        DatabaseKind::Postgres => Some("ctid::text"),
        _ => None,
    });
    if let Some(locator_expr) = locator_expr
        && let Some(plan) = editable_select_plan(sql)
    {
        return (
            build_editable_paginated_query(
                &plan,
                page_size,
                offset,
                locator_expr,
                filter,
                sort,
                dialect,
            ),
            true,
        );
    }

    (
        build_paginated_query(sql, page_size, offset, filter, sort, dialect),
        false,
    )
}

/// MySQL has no `rowid`/`ctid`; locators are `json_array` of PK columns.
/// Metadata lookup uses `QueryExec::locator_expression` — no `query` →
/// `driver-mysql` cycle.
async fn mysql_locator_expr(
    handle: &SessionHandle,
    sql: &str,
) -> Result<Option<String>, DatabaseError> {
    if handle.kind() != DatabaseKind::MySql || !is_paginated_query(sql) {
        return Ok(None);
    }
    let Some(plan) = editable_select_plan(sql) else {
        return Ok(None);
    };
    handle
        .query()
        .locator_expression(plan.source.schema.clone(), plan.source.table_name.clone())
        .await
}

fn with_editable_source(out: QueryOutput, original_sql: &str, used_locator: bool) -> QueryOutput {
    if !used_locator {
        return out;
    }
    let QueryOutput::Table(mut page) = out else {
        return out;
    };
    let Some(plan) = editable_select_plan(original_sql) else {
        return QueryOutput::Table(page);
    };
    if let Some(ctx) = page.editable.as_mut() {
        ctx.source = plan.source;
    } else {
        page.editable = Some(EditableTableContext {
            source: plan.source,
            row_locators: vec![String::new(); page.rows.len()],
        });
    }
    QueryOutput::Table(page)
}

fn is_paginated_query(sql: &str) -> bool {
    let keywords = statement_leading_keywords(sql);
    matches!(
        keywords.as_slice(),
        [keyword] if matches!(keyword.as_str(), "select" | "with")
    )
}

fn leading_sql_keyword(sql: &str) -> Option<String> {
    let bytes = sql.as_bytes();
    let mut index = 0;

    loop {
        while index < bytes.len()
            && (bytes[index].is_ascii_whitespace() || matches!(bytes[index], b'(' | b';'))
        {
            index += 1;
        }

        if index + 1 < bytes.len() && bytes[index] == b'-' && bytes[index + 1] == b'-' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }

        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }

        break;
    }

    let start = index;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_'))
    {
        index += 1;
    }

    (index > start).then(|| sql[start..index].to_ascii_lowercase())
}

fn statement_leading_keywords(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut quote = None::<u8>;

    while index < bytes.len() {
        if let Some(quote_byte) = quote {
            if bytes[index] == quote_byte {
                if quote_byte == b'\'' && index + 1 < bytes.len() && bytes[index + 1] == b'\'' {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            index += 1;
            continue;
        }

        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                quote = Some(bytes[index]);
                index += 1;
            }
            b'-' if index + 1 < bytes.len() && bytes[index + 1] == b'-' => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'*' => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b';' => {
                if let Some(keyword) = leading_sql_keyword(&sql[start..index]) {
                    statements.push(keyword);
                }
                start = index + 1;
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }

    if let Some(keyword) = leading_sql_keyword(&sql[start..]) {
        statements.push(keyword);
    }

    statements
}

/// Public-to-crate helper: return the leading SQL keyword of the first
/// statement in `sql`, or None. Reused by the splitter for read/write
/// classification.
pub(crate) fn leading_keyword(sql: &str) -> Option<String> {
    statement_leading_keywords(sql).into_iter().next()
}

fn rewrite_create_table_statement(
    create_statement: &str,
    replacement_qualified_name: &str,
) -> Result<String, DatabaseError> {
    let statement = create_statement.trim().trim_end_matches(';').trim();
    let lower = statement.to_ascii_lowercase();
    let create_table = "create table";
    let Some(create_index) = lower.find(create_table) else {
        return Err(DatabaseError::Unsupported(
            "Could not parse CREATE TABLE statement".to_string(),
        ));
    };

    let mut name_start = create_index + create_table.len();
    name_start = skip_sql_whitespace(statement, name_start);

    let if_not_exists = "if not exists";
    if lower[name_start..].starts_with(if_not_exists) {
        name_start += if_not_exists.len();
        name_start = skip_sql_whitespace(statement, name_start);
    }

    let Some(open_paren_offset) = statement[name_start..].find('(') else {
        return Err(DatabaseError::Unsupported(
            "Could not find the table definition in CREATE TABLE".to_string(),
        ));
    };
    let definition_start = name_start + open_paren_offset;

    Ok(format!(
        "{}{}{}",
        &statement[..name_start],
        replacement_qualified_name,
        &statement[definition_start..]
    ))
}

fn skip_sql_whitespace(sql: &str, mut index: usize) -> usize {
    while let Some(ch) = sql[index..].chars().next() {
        if ch.is_whitespace() {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    index
}

#[cfg(test)]
mod round_trip;

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        build_editable_paginated_query,
        create_table,
        drop_table,
        duplicate_table,
        editable_select_plan,
        execute_query,
        execute_query_page,
        is_read_only_sql,
        leading_sql_keyword,
        preview_source_for_sql,
        quote_identifier_clickhouse,
        rename_table,
        sql_for_query_exec,
        sql_literal,
        truncate_table,
    };
    use database::{
        DatabaseDriver,
        Dialect,
        FakeDriver,
        FormatFlavor,
        SessionHandle,
        quote_ident_backtick,
    };
    use driver_sqlite::{SqliteDriver, SqliteSession};
    use models::{DatabaseError, QueryOutput, TablePreviewSource};
    use std::sync::Arc;

    const MYSQL_DIALECT: Dialect = Dialect {
        quote_identifier: quote_ident_backtick,
        filter_expression: |_, _, _| "1=1".to_string(),
        format_flavor: FormatFlavor::Generic,
    };

    fn mysql_locator_expression(pk_columns: &[String]) -> String {
        let args = pk_columns
            .iter()
            .map(|column| format!("cast({} as char)", quote_identifier_clickhouse(column)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("json_array({args})")
    }

    fn parse_mysql_locator(
        locator: &str,
        pk_columns: &[String],
    ) -> Result<Vec<String>, DatabaseError> {
        let values = serde_json::from_str::<Vec<String>>(locator)
            .map_err(|_| DatabaseError::Unsupported("Invalid MySQL row locator".to_string()))?;
        if values.len() != pk_columns.len() {
            return Err(DatabaseError::Unsupported(
                "Invalid MySQL row locator".to_string(),
            ));
        }
        Ok(pk_columns
            .iter()
            .zip(values)
            .map(|(column, value)| {
                format!(
                    "cast({} as char) = {}",
                    quote_identifier_clickhouse(column),
                    sql_literal(&value)
                )
            })
            .collect())
    }

    async fn sqlite_handle() -> SessionHandle {
        let pool = SqliteDriver::connect(":memory:".into()).await.unwrap();
        SessionHandle::wrap(Arc::new(SqliteSession { pool }))
    }

    async fn exec_ok(handle: &SessionHandle, sql: &str) {
        execute_query(handle, sql.to_string()).await.unwrap();
    }

    async fn scalar_i64(handle: &SessionHandle, sql: &str) -> i64 {
        match execute_query(handle, sql.to_string()).await.unwrap() {
            QueryOutput::Table(page) => page.rows[0][0].parse().unwrap(),
            other => panic!("{other:?}"),
        }
    }

    async fn scalar_string(handle: &SessionHandle, sql: &str) -> String {
        match execute_query(handle, sql.to_string()).await.unwrap() {
            QueryOutput::Table(page) => page.rows[0][0].clone(),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_query_page_uses_fake_query_exec() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let out = execute_query_page(&handle, "select 1".into(), 10, 0, None, None)
            .await
            .unwrap();
        assert!(matches!(out, QueryOutput::Table(_)));
    }

    #[test]
    fn read_only_sql_detection_matches_supported_queries() {
        assert!(is_read_only_sql("select * from products"));
        assert!(is_read_only_sql(
            "WITH recent AS (select 1) select * from recent"
        ));
        assert!(is_read_only_sql("show tables"));
        assert!(is_read_only_sql("pragma table_info(products)"));
        assert!(!is_read_only_sql("update products set price = 10"));
        assert!(!is_read_only_sql("delete from products"));
    }

    #[test]
    fn mysql_locator_round_trip_uses_json_array_encoding() {
        let locator = r#"["42","tenant-a"]"#;
        let pk_columns = vec!["id".to_string(), "tenant_id".to_string()];

        assert_eq!(
            mysql_locator_expression(&pk_columns),
            "json_array(cast(`id` as char), cast(`tenant_id` as char))"
        );
        assert_eq!(
            parse_mysql_locator(locator, &pk_columns).unwrap(),
            vec![
                "cast(`id` as char) = '42'",
                "cast(`tenant_id` as char) = 'tenant-a'"
            ]
        );
    }

    #[test]
    fn mysql_select_star_sql_includes_json_array_locator_column() {
        let plan = editable_select_plan("select * from t").unwrap();
        let locator = mysql_locator_expression(&["id".to_string()]);
        let built =
            build_editable_paginated_query(&plan, 10, 0, &locator, None, None, MYSQL_DIALECT);
        assert!(
            built.contains(r#"json_array(cast(`id` as char)) as "__shovel_locator""#),
            "expected locator column in {built}"
        );

        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        let (exec_sql, used_locator) = sql_for_query_exec(
            &handle,
            "select * from t",
            10,
            0,
            None,
            None,
            Some(locator.as_str()),
        );
        assert!(used_locator);
        assert!(
            exec_sql.contains(r#"json_array(cast(`id` as char)) as "__shovel_locator""#),
            "expected locator column in {exec_sql}"
        );
    }

    #[tokio::test]
    async fn execute_query_page_supports_quoted_sqlite_table_names() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null,
                price real not null
            );
            "#,
        )
        .await;
        exec_ok(
            &handle,
            r#"
            insert into "products" (name, price)
            values
                ('Wireless Mouse', 29.99),
                ('Mechanical Keyboard', 89.99);
            "#,
        )
        .await;

        let result = execute_query_page(
            &handle,
            r#"select * from "products" limit 100;"#.to_string(),
            100,
            0,
            None,
            None,
        )
        .await
        .unwrap();

        match result {
            QueryOutput::Table(page) => {
                assert_eq!(page.columns, vec!["id", "name", "price"]);
                assert_eq!(page.rows.len(), 2);
                assert_eq!(page.rows[0][1], "Wireless Mouse");
                assert!(page.editable.is_some());
            }
            other => panic!("expected table result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_table_creates_sqlite_table() {
        let handle = sqlite_handle().await;

        create_table(
            &handle,
            Some("main".to_string()),
            "products".to_string(),
            "id integer primary key,\nname text not null".to_string(),
            None,
        )
        .await
        .unwrap();

        let remaining = scalar_i64(
            &handle,
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .await;

        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn drop_table_removes_sqlite_table() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .await;

        drop_table(
            &handle,
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
        )
        .await
        .unwrap();

        let remaining = scalar_i64(
            &handle,
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .await;

        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn truncate_table_clears_sqlite_rows_without_dropping_table() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .await;
        exec_ok(
            &handle,
            r#"insert into "products" (name) values ('Keyboard'), ('Mouse');"#,
        )
        .await;

        truncate_table(
            &handle,
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
        )
        .await
        .unwrap();

        let remaining_rows = scalar_i64(&handle, r#"select count(*) from "products""#).await;
        let remaining_tables = scalar_i64(
            &handle,
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .await;

        assert_eq!(remaining_rows, 0);
        assert_eq!(remaining_tables, 1);
    }

    #[tokio::test]
    async fn rename_table_renames_sqlite_table() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .await;

        rename_table(
            &handle,
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
            "inventory".to_string(),
        )
        .await
        .unwrap();

        let remaining = scalar_i64(
            &handle,
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'inventory'
            "#,
        )
        .await;
        let old_remaining = scalar_i64(
            &handle,
            r#"
            select count(*)
            from sqlite_master
            where type = 'table'
              and name = 'products'
            "#,
        )
        .await;

        assert_eq!(remaining, 1);
        assert_eq!(old_remaining, 0);
    }

    #[tokio::test]
    async fn duplicate_table_creates_sqlite_copy_with_rows() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .await;
        exec_ok(
            &handle,
            r#"insert into "products" (name) values ('Keyboard'), ('Mouse');"#,
        )
        .await;

        duplicate_table(
            &handle,
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
            "products_copy".to_string(),
            true,
        )
        .await
        .unwrap();

        let copy_rows = scalar_i64(&handle, r#"select count(*) from "products_copy""#).await;
        let copied_create_sql = scalar_string(
            &handle,
            r#"
            select sql
            from sqlite_master
            where type = 'table'
              and name = 'products_copy'
            "#,
        )
        .await;

        assert_eq!(copy_rows, 2);
        assert!(copied_create_sql.contains("products_copy"));
    }

    #[tokio::test]
    async fn duplicate_table_can_copy_structure_only_for_sqlite() {
        let handle = sqlite_handle().await;
        exec_ok(
            &handle,
            r#"
            create table "products" (
                id integer primary key,
                name text not null
            );
            "#,
        )
        .await;
        exec_ok(
            &handle,
            r#"insert into "products" (name) values ('Keyboard');"#,
        )
        .await;

        duplicate_table(
            &handle,
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: r#""products""#.to_string(),
            },
            "products_empty_copy".to_string(),
            false,
        )
        .await
        .unwrap();

        let copy_rows = scalar_i64(&handle, r#"select count(*) from "products_empty_copy""#).await;

        assert_eq!(copy_rows, 0);
    }

    #[test]
    fn infers_preview_source_for_simple_select() {
        let source = preview_source_for_sql(r#"select id, name from "main"."products" limit 100"#)
            .expect("source");

        assert_eq!(source.schema.as_deref(), Some("main"));
        assert_eq!(source.table_name, "products");
        assert_eq!(source.qualified_name, r#""main"."products""#);
    }

    #[test]
    fn skips_preview_source_for_join_query() {
        assert!(
            preview_source_for_sql(
                "select p.id from products p join categories c on c.id = p.category_id"
            )
            .is_none()
        );
    }

    #[test]
    fn leading_keyword_extracts_first_sql_word() {
        assert_eq!(leading_sql_keyword("SELECT 1"), Some("select".to_string()));
        assert_eq!(
            leading_sql_keyword("insert into t values (1)"),
            Some("insert".to_string())
        );
        assert_eq!(
            leading_sql_keyword("  update t set x = 1"),
            Some("update".to_string())
        );
        assert_eq!(
            leading_sql_keyword("-- comment\nselect 1"),
            Some("select".to_string())
        );
        assert_eq!(
            leading_sql_keyword("/* comment */\nselect 1"),
            Some("select".to_string())
        );
        assert_eq!(leading_sql_keyword(""), None);
        assert_eq!(leading_sql_keyword("   "), None);
    }

    #[test]
    fn is_read_only_sql_gates_dispatch_for_keyboard_shortcut_triggers() {
        assert!(is_read_only_sql("select * from users"));
        assert!(is_read_only_sql("explain select * from users"));
        assert!(is_read_only_sql("describe users"));
        assert!(is_read_only_sql("show tables"));
        assert!(is_read_only_sql("WITH cte AS (select 1) select * from cte"));
        assert!(is_read_only_sql("pragma table_info(users)"));
        assert!(!is_read_only_sql(
            "insert into users (name) values ('test')"
        ));
        assert!(!is_read_only_sql("update users set name = 'test'"));
        assert!(!is_read_only_sql("delete from users"));
        assert!(!is_read_only_sql("drop table users"));
        assert!(!is_read_only_sql("alter table users add column email text"));
        assert!(!is_read_only_sql("select 1; drop table users"));
        assert!(is_read_only_sql("select '; drop table users' as text"));
    }
}

fn qualified_sqlite_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema),
            quote_identifier(table_name)
        ),
        None => quote_identifier(table_name),
    }
}

fn qualified_postgres_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema),
            quote_identifier(table_name)
        ),
        None => quote_identifier(table_name),
    }
}

fn qualified_mysql_table_name(schema: Option<&str>, table_name: &str) -> String {
    match schema.map(str::trim).filter(|schema| !schema.is_empty()) {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier_clickhouse(schema),
            quote_identifier_clickhouse(table_name)
        ),
        None => quote_identifier_clickhouse(table_name),
    }
}
