// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{SchemaExec, quote_ident_double};
use models::{DatabaseError, ExplorerNode, ExplorerNodeKind, QueryOutput, TableForeignKey};
use sqlx::Row;

use crate::session::SqliteSession;

#[async_trait]
impl SchemaExec for SqliteSession {
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
    pool: &sqlx::SqlitePool,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| "main".to_string());
    let mut rows = Vec::new();

    let table_sql = format!(
        "select sql from {}.sqlite_master where type in ('table', 'view') and name = ?1",
        quote_ident_double(&schema_name)
    );
    if let Some(create_sql) = sqlx::query_scalar::<_, Option<String>>(&table_sql)
        .bind(&table)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
        .flatten()
    {
        rows.push(structure_row(
            "table",
            table.clone(),
            "definition",
            String::new(),
            create_sql,
        ));
    }

    let columns_sql = format!(
        "PRAGMA {}.table_info({})",
        quote_ident_double(&schema_name),
        quote_ident_double(&table)
    );
    let column_rows = sqlx::query(&columns_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in column_rows {
        let column_name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let data_type = row
            .try_get::<String, _>("type")
            .unwrap_or_else(|_| "TEXT".to_string());
        let not_null = row.try_get::<i64, _>("notnull").unwrap_or(0) == 1;
        let default_value = row
            .try_get::<Option<String>, _>("dflt_value")
            .ok()
            .flatten();
        let pk_position = row.try_get::<i64, _>("pk").unwrap_or(0);
        rows.push(structure_row(
            "column",
            column_name,
            data_type,
            if pk_position > 0 {
                format!("pk#{pk_position}")
            } else {
                String::new()
            },
            sqlite_column_details(not_null, default_value),
        ));
    }

    let index_sql = format!(
        "PRAGMA {}.index_list({})",
        quote_ident_double(&schema_name),
        quote_ident_double(&table)
    );
    let index_rows = sqlx::query(&index_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in index_rows {
        let index_name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let unique = row.try_get::<i64, _>("unique").unwrap_or(0) == 1;
        let origin = row
            .try_get::<String, _>("origin")
            .unwrap_or_else(|_| String::new());
        let partial = row.try_get::<i64, _>("partial").unwrap_or(0) == 1;
        let index_columns = load_sqlite_index_columns(pool, &schema_name, &index_name).await?;
        let create_sql = sqlx::query_scalar::<_, Option<String>>(&format!(
            "select sql from {}.sqlite_master where type = 'index' and name = ?1",
            quote_ident_double(&schema_name)
        ))
        .bind(&index_name)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
        .flatten()
        .unwrap_or_default();

        rows.push(structure_row(
            "index",
            index_name,
            if unique { "UNIQUE" } else { "INDEX" }.to_string(),
            index_columns.join(", "),
            join_non_empty([
                (!origin.is_empty()).then(|| format!("origin: {origin}")),
                partial.then(|| "partial".to_string()),
                (!create_sql.is_empty()).then_some(create_sql),
            ]),
        ));
    }

    let foreign_key_sql = format!(
        "PRAGMA {}.foreign_key_list({})",
        quote_ident_double(&schema_name),
        quote_ident_double(&table)
    );
    let foreign_key_rows = sqlx::query(&foreign_key_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in foreign_key_rows {
        let id = row.try_get::<i64, _>("id").unwrap_or_default();
        let from_column = row
            .try_get::<String, _>("from")
            .unwrap_or_else(|_| String::new());
        let target_table = row
            .try_get::<String, _>("table")
            .unwrap_or_else(|_| String::new());
        let target_column = row
            .try_get::<String, _>("to")
            .unwrap_or_else(|_| String::new());
        let on_update = row
            .try_get::<String, _>("on_update")
            .unwrap_or_else(|_| String::new());
        let on_delete = row
            .try_get::<String, _>("on_delete")
            .unwrap_or_else(|_| String::new());

        rows.push(structure_row(
            "constraint",
            format!("fk_{id}_{from_column}"),
            "FOREIGN KEY",
            format!("{from_column} -> {target_table}.{target_column}"),
            join_non_empty([
                (!on_update.is_empty()).then(|| format!("on update {on_update}")),
                (!on_delete.is_empty()).then(|| format!("on delete {on_delete}")),
            ]),
        ));
    }

    let trigger_sql = format!(
        "select name, sql from {}.sqlite_master where type = 'trigger' and tbl_name = ?1 order by name",
        quote_ident_double(&schema_name)
    );
    let trigger_rows = sqlx::query(&trigger_sql)
        .bind(&table)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in trigger_rows {
        let trigger_name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let sql = row
            .try_get::<Option<String>, _>("sql")
            .ok()
            .flatten()
            .unwrap_or_default();
        rows.push(structure_row(
            "trigger",
            trigger_name,
            "TRIGGER",
            String::new(),
            sql,
        ));
    }

    Ok(QueryOutput::Table(structure_page(rows)))
}

async fn load_table_columns(
    pool: &sqlx::SqlitePool,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| "main".to_string());
    let sql = format!(
        "PRAGMA {}.table_info({})",
        quote_ident_double(&schema_name),
        quote_ident_double(&table)
    );

    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("name")
                .map_err(|e| DatabaseError::Driver(e.to_string()))
        })
        .collect()
}

async fn load_connection_tree(pool: &sqlx::SqlitePool) -> Result<Vec<ExplorerNode>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select name, type
        from sqlite_master
        where type in ('table', 'view', 'trigger')
          and name not like 'sqlite_%'
        order by type, name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut tables = Vec::new();
    let mut views = Vec::new();
    let mut triggers = Vec::new();

    for row in rows {
        let name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let kind = row
            .try_get::<String, _>("type")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;

        let node = ExplorerNode {
            qualified_name: quote_ident_double(&name),
            schema: Some("main".to_string()),
            name,
            kind: match kind.as_str() {
                "table" => ExplorerNodeKind::Table,
                "view" => ExplorerNodeKind::View,
                "trigger" => ExplorerNodeKind::Trigger,
                _ => continue,
            },
            row_count: None,
            children: Vec::new(),
        };
        let node = if node.kind == ExplorerNodeKind::Table {
            ExplorerNode {
                row_count: sqlite_table_row_count(pool, &node.name).await,
                ..node
            }
        } else {
            node
        };
        match node.kind {
            ExplorerNodeKind::Table => tables.push(node),
            ExplorerNodeKind::View => views.push(node),
            ExplorerNodeKind::Trigger => triggers.push(node),
            _ => {}
        }
    }

    Ok(vec![ExplorerNode {
        name: "main".to_string(),
        kind: ExplorerNodeKind::Schema,
        schema: Some("main".to_string()),
        qualified_name: "main".to_string(),
        row_count: None,
        children: tables.into_iter().chain(views).chain(triggers).collect(),
    }])
}

/// Best-effort row count for a SQLite table. SQLite is a local DB, so a full
/// `count(*)` is acceptable. Any error (including views/triggers being asked
/// for a count) yields `None` — never fails the tree.
async fn sqlite_table_row_count(pool: &sqlx::SqlitePool, table: &str) -> Option<u64> {
    let sql = format!("select count(*) from {}", quote_ident_double(table));
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .ok()
        .and_then(|count| u64::try_from(count).ok())
}

/// Загружает все внешние ключи базы SQLite. У SQLite нет единого каталога
/// FK, поэтому перебираем таблицы из `sqlite_master` и для каждой вызываем
/// `PRAGMA foreign_key_list`. Схема FK-цели в SQLite всегда совпадает со
/// схемой источника (межбазовые FK через ATTACH не поддерживаются).
async fn load_foreign_keys(pool: &sqlx::SqlitePool) -> Result<Vec<TableForeignKey>, DatabaseError> {
    let schema = "main".to_string();
    let table_rows = sqlx::query(
        r#"
        select name
        from sqlite_master
        where type = 'table'
          and name not like 'sqlite_%'
        order by name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut foreign_keys = Vec::new();
    for table_row in table_rows {
        let from_table = table_row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;

        let pragma = format!(
            "PRAGMA {}.foreign_key_list({})",
            quote_ident_double(&schema),
            quote_ident_double(&from_table)
        );
        let fk_rows = sqlx::query(&pragma)
            .fetch_all(pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;

        for fk_row in fk_rows {
            let id = fk_row.try_get::<i64, _>("id").unwrap_or_default();
            let from_column = fk_row.try_get::<String, _>("from").unwrap_or_default();
            let to_table = fk_row.try_get::<String, _>("table").unwrap_or_default();
            // Колонка-цель может быть пустой для составных FK в старых SQLite.
            let to_column = fk_row
                .try_get::<Option<String>, _>("to")
                .ok()
                .flatten()
                .unwrap_or_default();

            foreign_keys.push(TableForeignKey {
                name: format!("fk_{id}_{from_table}_{from_column}"),
                from_schema: schema.clone(),
                from_table: from_table.clone(),
                from_column,
                to_schema: schema.clone(),
                to_table,
                to_column,
            });
        }
    }

    Ok(foreign_keys)
}

/// Возвращает исходный DDL объекта (таблицы или представления) из `sqlite_master`.
/// `None`, если объект не найден или у него нет SQL (например, auto-index).
async fn load_object_ddl(
    pool: &sqlx::SqlitePool,
    schema: Option<String>,
    object: String,
    kind: ExplorerNodeKind,
) -> Result<Option<String>, DatabaseError> {
    // SQLite хранит DDL для таблиц, представлений и триггеров в sqlite_master.
    // Прочие типы объектов (последовательности, функции и т.п.) в SQLite отсутствуют.
    let type_filter = match kind {
        ExplorerNodeKind::Table | ExplorerNodeKind::View => "('table', 'view')",
        ExplorerNodeKind::Trigger => "('trigger')",
        _ => return Ok(None),
    };
    let schema_name = schema.unwrap_or_else(|| "main".to_string());
    let sql = format!(
        "select sql from {}.sqlite_master where type in {type_filter} and name = ?1",
        quote_ident_double(&schema_name)
    );
    let ddl = sqlx::query_scalar::<_, Option<String>>(&sql)
        .bind(&object)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?
        .flatten();
    Ok(ddl.filter(|s| !s.trim().is_empty()))
}

async fn load_sqlite_index_columns(
    pool: &sqlx::SqlitePool,
    schema_name: &str,
    index_name: &str,
) -> Result<Vec<String>, DatabaseError> {
    let sql = format!(
        "PRAGMA {}.index_info({})",
        quote_ident_double(schema_name),
        quote_ident_double(index_name)
    );
    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("name").ok())
        .collect())
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

fn sqlite_column_details(not_null: bool, default_value: Option<String>) -> String {
    join_non_empty([
        not_null.then(|| "NOT NULL".to_string()),
        default_value.map(|value| format!("default {value}")),
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
