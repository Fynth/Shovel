// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{SchemaExec, quote_ident_backtick};
use models::{DatabaseError, ExplorerNode, ExplorerNodeKind, QueryOutput, TableForeignKey};
use sqlx::Row;

use crate::session::{MysqlSession, mysql_effective_schema_name};

#[async_trait]
impl SchemaExec for MysqlSession {
    async fn describe_table(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        describe_table(&self.pool, schema, table).await
    }

    async fn load_table_columns(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        load_table_columns(&self.pool, schema, table).await
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        load_connection_tree(&self.pool).await
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        load_foreign_keys(&self.pool).await
    }

    async fn load_object_ddl(
        &self,
        schema: Option<String>,
        object: String,
        kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        load_object_ddl(&self.pool, schema, object, kind).await
    }
}

async fn describe_table(
    pool: &sqlx::MySqlPool,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    let schema_name = mysql_effective_schema_name(pool, schema.as_deref()).await?;
    let mut rows = Vec::new();

    let overview_rows = sqlx::query(
        r#"
        select table_type, engine
        from information_schema.tables
        where table_schema = ?
          and table_name = ?
        limit 1
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    if let Some(row) = overview_rows.first() {
        let table_type = row
            .try_get::<String, _>("table_type")
            .unwrap_or_else(|_| "TABLE".to_string());
        let engine = row
            .try_get::<Option<String>, _>("engine")
            .ok()
            .flatten()
            .unwrap_or_else(|| table_type.clone());
        rows.push(structure_row(
            "table",
            table.clone(),
            engine,
            String::new(),
            format!("schema: {schema_name}"),
        ));
    }

    let create_sql = format!(
        "show create table {}",
        qualified_mysql_table_name(&schema_name, &table)
    );
    if let Some(row) = sqlx::query(&create_sql)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
    {
        let create_statement = row
            .try_get::<String, _>(1)
            .or_else(|_| row.try_get::<String, _>("Create Table"))
            .or_else(|_| row.try_get::<String, _>("Create View"))
            .unwrap_or_default();
        if !create_statement.trim().is_empty() {
            rows.push(structure_row(
                "table",
                table.clone(),
                "definition".to_string(),
                String::new(),
                create_statement,
            ));
        }
    }

    let column_rows = sqlx::query(
        r#"
        select column_name, column_type, is_nullable, column_default, extra
        from information_schema.columns
        where table_schema = ?
          and table_name = ?
        order by ordinal_position
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in column_rows {
        let column_name = row
            .try_get::<String, _>("column_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let column_type = row
            .try_get::<String, _>("column_type")
            .unwrap_or_else(|_| "text".to_string());
        let is_nullable = row
            .try_get::<String, _>("is_nullable")
            .unwrap_or_else(|_| "YES".to_string());
        let default_value = row
            .try_get::<Option<String>, _>("column_default")
            .ok()
            .flatten();
        let extra = row
            .try_get::<Option<String>, _>("extra")
            .ok()
            .flatten()
            .unwrap_or_default();
        rows.push(structure_row(
            "column",
            column_name,
            column_type,
            String::new(),
            mysql_column_details(&is_nullable, default_value, &extra),
        ));
    }

    let index_rows = sqlx::query(
        r#"
        select index_name, non_unique, index_type, seq_in_index, column_name
        from information_schema.statistics
        where table_schema = ?
          and table_name = ?
        order by index_name, seq_in_index
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    let mut grouped_indexes: std::collections::BTreeMap<String, (bool, String, Vec<String>)> =
        std::collections::BTreeMap::new();
    for row in index_rows {
        let index_name = row
            .try_get::<String, _>("index_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let non_unique = row.try_get::<i64, _>("non_unique").unwrap_or(1) != 0;
        let index_type = row
            .try_get::<Option<String>, _>("index_type")
            .ok()
            .flatten()
            .unwrap_or_else(|| "INDEX".to_string());
        let column_name = row
            .try_get::<Option<String>, _>("column_name")
            .ok()
            .flatten()
            .unwrap_or_default();
        let entry =
            grouped_indexes
                .entry(index_name)
                .or_insert((non_unique, index_type, Vec::new()));
        if !column_name.is_empty() {
            entry.2.push(column_name);
        }
    }
    for (index_name, (non_unique, index_type, columns)) in grouped_indexes {
        rows.push(structure_row(
            "index",
            index_name,
            if non_unique {
                index_type
            } else {
                format!("UNIQUE {index_type}")
            },
            columns.join(", "),
            String::new(),
        ));
    }

    let constraint_rows = sqlx::query(
        r#"
        select
          tc.constraint_name,
          tc.constraint_type,
          kcu.column_name,
          kcu.referenced_table_schema,
          kcu.referenced_table_name,
          kcu.referenced_column_name,
          kcu.ordinal_position
        from information_schema.table_constraints tc
        left join information_schema.key_column_usage kcu
          on tc.constraint_name = kcu.constraint_name
         and tc.table_schema = kcu.table_schema
         and tc.table_name = kcu.table_name
        where tc.table_schema = ?
          and tc.table_name = ?
        order by tc.constraint_name, kcu.ordinal_position
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    let mut grouped_constraints: std::collections::BTreeMap<
        String,
        (String, Vec<String>, Vec<String>),
    > = std::collections::BTreeMap::new();
    for row in constraint_rows {
        let constraint_name = row
            .try_get::<String, _>("constraint_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let constraint_type = row
            .try_get::<String, _>("constraint_type")
            .unwrap_or_else(|_| "CONSTRAINT".to_string());
        let column_name = row
            .try_get::<Option<String>, _>("column_name")
            .ok()
            .flatten();
        let referenced_schema = row
            .try_get::<Option<String>, _>("referenced_table_schema")
            .ok()
            .flatten();
        let referenced_table = row
            .try_get::<Option<String>, _>("referenced_table_name")
            .ok()
            .flatten();
        let referenced_column = row
            .try_get::<Option<String>, _>("referenced_column_name")
            .ok()
            .flatten();

        let entry = grouped_constraints.entry(constraint_name).or_insert((
            constraint_type,
            Vec::new(),
            Vec::new(),
        ));
        if let Some(column_name) = column_name {
            entry.1.push(column_name);
        }
        if let Some(referenced_table) = referenced_table {
            let referenced_schema = referenced_schema.unwrap_or_default();
            let referenced_column = referenced_column.unwrap_or_default();
            entry.2.push(if referenced_schema.is_empty() {
                format!("{referenced_table}.{referenced_column}")
            } else {
                format!("{referenced_schema}.{referenced_table}.{referenced_column}")
            });
        }
    }
    for (constraint_name, (constraint_type, columns, references)) in grouped_constraints {
        rows.push(structure_row(
            "constraint",
            constraint_name,
            constraint_type,
            columns.join(", "),
            references.join(", "),
        ));
    }

    let trigger_rows = sqlx::query(
        r#"
        select trigger_name, action_timing, event_manipulation, action_statement
        from information_schema.triggers
        where trigger_schema = ?
          and event_object_schema = ?
          and event_object_table = ?
        order by trigger_name
        "#,
    )
    .bind(&schema_name)
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in trigger_rows {
        let trigger_name = row
            .try_get::<String, _>("trigger_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let timing = row
            .try_get::<String, _>("action_timing")
            .unwrap_or_else(|_| String::new());
        let event = row
            .try_get::<String, _>("event_manipulation")
            .unwrap_or_else(|_| String::new());
        let action = row
            .try_get::<String, _>("action_statement")
            .unwrap_or_else(|_| String::new());
        rows.push(structure_row(
            "trigger",
            trigger_name,
            join_non_empty([
                (!timing.is_empty()).then_some(timing),
                (!event.is_empty()).then_some(event),
            ]),
            String::new(),
            action,
        ));
    }

    Ok(QueryOutput::Table(structure_page(rows)))
}

/// Загружает все внешние ключи пользовательских баз MySQL.
/// `information_schema.key_column_usage` даёт по строке на колонку FK
/// (для составных FK `constraint_name` повторяется — каждая пара колонок
/// становится отдельной связью на диаграмме). Системные базы отбрасываем.
async fn load_foreign_keys(pool: &sqlx::MySqlPool) -> Result<Vec<TableForeignKey>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select
          constraint_name,
          table_schema  as from_schema,
          table_name    as from_table,
          column_name   as from_column,
          referenced_table_schema as to_schema,
          referenced_table_name   as to_table,
          referenced_column_name  as to_column
        from information_schema.key_column_usage
        where referenced_table_name is not null
          and table_schema not in ('information_schema', 'mysql', 'performance_schema', 'sys')
        order by table_schema, table_name, ordinal_position
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut foreign_keys = Vec::new();
    for row in rows {
        foreign_keys.push(TableForeignKey {
            name: row
                .try_get::<String, _>("constraint_name")
                .unwrap_or_default(),
            from_schema: row.try_get::<String, _>("from_schema").unwrap_or_default(),
            from_table: row.try_get::<String, _>("from_table").unwrap_or_default(),
            from_column: row.try_get::<String, _>("from_column").unwrap_or_default(),
            to_schema: row
                .try_get::<Option<String>, _>("to_schema")
                .ok()
                .flatten()
                .unwrap_or_default(),
            to_table: row
                .try_get::<Option<String>, _>("to_table")
                .ok()
                .flatten()
                .unwrap_or_default(),
            to_column: row
                .try_get::<Option<String>, _>("to_column")
                .ok()
                .flatten()
                .unwrap_or_default(),
        });
    }
    Ok(foreign_keys)
}

/// Возвращает DDL объекта через `SHOW CREATE TABLE` (работает и для таблиц,
/// и для представлений — колонка результата `Create Table`/`Create View`).
/// `None`, если объект не найден.
async fn load_object_ddl(
    pool: &sqlx::MySqlPool,
    schema: Option<String>,
    object: String,
    kind: ExplorerNodeKind,
) -> Result<Option<String>, DatabaseError> {
    let schema_name = mysql_effective_schema_name(pool, schema.as_deref()).await?;
    match kind {
        ExplorerNodeKind::Function => {
            let sql = format!(
                "show create function {}",
                qualified_mysql_table_name(&schema_name, &object)
            );
            fetch_mysql_create_ddl(pool, &sql, &["Create Function"]).await
        }
        ExplorerNodeKind::Procedure => {
            let sql = format!(
                "show create procedure {}",
                qualified_mysql_table_name(&schema_name, &object)
            );
            fetch_mysql_create_ddl(pool, &sql, &["Create Procedure"]).await
        }
        ExplorerNodeKind::Trigger => {
            let sql = format!(
                "show create trigger {}",
                qualified_mysql_table_name(&schema_name, &object)
            );
            fetch_mysql_create_ddl(pool, &sql, &["SQL Original Statement"]).await
        }
        // Таблицы, представления и неизвестные типы — SHOW CREATE TABLE.
        ExplorerNodeKind::Table
        | ExplorerNodeKind::View
        | ExplorerNodeKind::MaterializedView
        | ExplorerNodeKind::Sequence
        | ExplorerNodeKind::Schema
        | ExplorerNodeKind::Column => {
            let sql = format!(
                "show create table {}",
                qualified_mysql_table_name(&schema_name, &object)
            );
            fetch_mysql_create_ddl(pool, &sql, &["Create Table", "Create View"]).await
        }
    }
}

/// Выполняет `SHOW CREATE ...` и достаёт тело DDL: сначала пробует
/// именованные колонки, затем колонку по индексу 2 (типичный layout
/// `SHOW CREATE` в MySQL). `None`, если строк нет или тело пустое.
async fn fetch_mysql_create_ddl(
    pool: &sqlx::MySqlPool,
    sql: &str,
    named_columns: &[&str],
) -> Result<Option<String>, DatabaseError> {
    let Some(row) = sqlx::query(sql)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
    else {
        return Ok(None);
    };
    let mut ddl = String::new();
    for name in named_columns {
        if let Ok(value) = row.try_get::<String, _>(name) {
            ddl = value;
            break;
        }
    }
    if ddl.trim().is_empty() {
        // fallback: колонка по индексу 2 (третья), где MySQL кладёт тело.
        ddl = row.try_get::<String, _>(2usize).unwrap_or_default();
    }
    Ok(if ddl.trim().is_empty() {
        None
    } else {
        Some(ddl)
    })
}

async fn load_table_columns(
    pool: &sqlx::MySqlPool,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    let schema_name = mysql_effective_schema_name(pool, schema.as_deref()).await?;
    let rows = sqlx::query(
        r#"
        select column_name
        from information_schema.columns
        where table_schema = ?
          and table_name = ?
        order by ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("column_name")
                .map_err(|e| DatabaseError::Driver(e.to_string()))
        })
        .collect()
}

async fn load_connection_tree(pool: &sqlx::MySqlPool) -> Result<Vec<ExplorerNode>, DatabaseError> {
    let mut grouped: std::collections::BTreeMap<String, Vec<ExplorerNode>> =
        std::collections::BTreeMap::new();

    let push_node = |grouped: &mut std::collections::BTreeMap<String, Vec<ExplorerNode>>,
                     schema: String,
                     name: String,
                     kind: ExplorerNodeKind,
                     row_count: Option<u64>| {
        grouped
            .entry(schema.clone())
            .or_default()
            .push(ExplorerNode {
                qualified_name: qualified_mysql_table_name(&schema, &name),
                schema: Some(schema),
                name,
                kind,
                row_count,
                children: Vec::new(),
            });
    };

    // Таблицы и представления. Для таблиц дополнительно достаём оценку
    // числа строк из `information_schema.tables.TABLE_ROWS` (статистика,
    // дешевле полного `COUNT(*)`).
    let rows = sqlx::query(
        r#"
        select table_schema, table_name, table_type, table_rows
        from information_schema.tables
        where table_schema not in ('information_schema', 'performance_schema', 'sys', 'mysql')
        order by table_schema, table_type, table_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("table_schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("table_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let table_type = row
            .try_get::<String, _>("table_type")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let kind = if table_type.eq_ignore_ascii_case("view") {
            ExplorerNodeKind::View
        } else {
            ExplorerNodeKind::Table
        };
        let row_count = match kind {
            ExplorerNodeKind::Table => row
                .try_get::<i64, _>("table_rows")
                .ok()
                .filter(|count| *count >= 0)
                .and_then(|count| u64::try_from(count).ok()),
            _ => None,
        };
        push_node(&mut grouped, schema, name, kind, row_count);
    }

    // Функции и процедуры.
    let rows = sqlx::query(
        r#"
        select routine_schema, routine_name, routine_type
        from information_schema.routines
        where routine_schema not in ('information_schema', 'performance_schema', 'sys', 'mysql')
        order by routine_schema, routine_type, routine_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("routine_schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("routine_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let routine_type = row.try_get::<String, _>("routine_type").unwrap_or_default();
        let kind = if routine_type.eq_ignore_ascii_case("procedure") {
            ExplorerNodeKind::Procedure
        } else {
            ExplorerNodeKind::Function
        };
        push_node(&mut grouped, schema, name, kind, None);
    }

    // Триггеры.
    let rows = sqlx::query(
        r#"
        select trigger_schema, trigger_name
        from information_schema.triggers
        where trigger_schema not in ('information_schema', 'performance_schema', 'sys', 'mysql')
        order by trigger_schema, trigger_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("trigger_schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("trigger_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        push_node(&mut grouped, schema, name, ExplorerNodeKind::Trigger, None);
    }

    Ok(grouped
        .into_iter()
        .map(|(schema, children)| ExplorerNode {
            qualified_name: quote_ident_backtick(&schema),
            schema: Some(schema.clone()),
            name: schema,
            kind: ExplorerNodeKind::Schema,
            row_count: None,
            children,
        })
        .collect())
}

fn qualified_mysql_table_name(schema_name: &str, table_name: &str) -> String {
    format!(
        "{}.{}",
        quote_ident_backtick(schema_name),
        quote_ident_backtick(table_name)
    )
}

fn structure_row(
    section: impl Into<String>,
    name: impl Into<String>,
    row_type: impl Into<String>,
    target: impl Into<String>,
    details: impl Into<String>,
) -> Vec<String> {
    vec![
        section.into(),
        name.into(),
        row_type.into(),
        target.into(),
        details.into(),
    ]
}

fn structure_page(rows: Vec<Vec<String>>) -> models::QueryPage {
    models::QueryPage {
        columns: vec![
            "section".to_string(),
            "name".to_string(),
            "type".to_string(),
            "target".to_string(),
            "details".to_string(),
        ],
        rows,
        editable: None,
        offset: 0,
        page_size: 0,
        has_previous: false,
        has_next: false,
    }
}

fn mysql_column_details(is_nullable: &str, default_value: Option<String>, extra: &str) -> String {
    join_non_empty([
        is_nullable
            .eq_ignore_ascii_case("NO")
            .then(|| "NOT NULL".to_string()),
        default_value.map(|value| format!("default {value}")),
        (!extra.trim().is_empty()).then(|| extra.trim().to_string()),
    ])
}

fn join_non_empty(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}
