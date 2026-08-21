use models::{DatabaseError, ExplorerNode, ExplorerNodeKind, QueryOutput, TableForeignKey};
use sqlx::Row;

pub async fn describe_table_postgres(
    pool: &sqlx::PgPool,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| "public".to_string());
    let mut rows = Vec::new();

    let column_rows = sqlx::query(
        r#"
        select
          ordinal_position,
          column_name,
          data_type,
          is_nullable,
          column_default
        from information_schema.columns
        where table_schema = $1
          and table_name = $2
        order by ordinal_position
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;
    for row in column_rows {
        let column_name = row
            .try_get::<String, _>("column_name")
            .map_err(DatabaseError::Postgres)?;
        let data_type = row
            .try_get::<String, _>("data_type")
            .unwrap_or_else(|_| "text".to_string());
        let is_nullable = row
            .try_get::<String, _>("is_nullable")
            .unwrap_or_else(|_| "YES".to_string());
        let default_value = row
            .try_get::<Option<String>, _>("column_default")
            .ok()
            .flatten();

        rows.push(structure_row(
            "column",
            column_name,
            data_type,
            String::new(),
            postgres_column_details(&is_nullable, default_value),
        ));
    }

    let index_rows = sqlx::query(
        r#"
        select indexname, indexdef
        from pg_indexes
        where schemaname = $1
          and tablename = $2
        order by indexname
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;
    for row in index_rows {
        let index_name = row
            .try_get::<String, _>("indexname")
            .map_err(DatabaseError::Postgres)?;
        let index_definition = row
            .try_get::<String, _>("indexdef")
            .unwrap_or_else(|_| String::new());
        rows.push(structure_row(
            "index",
            index_name,
            if index_definition.contains(" UNIQUE INDEX ") {
                "UNIQUE".to_string()
            } else {
                "INDEX".to_string()
            },
            String::new(),
            index_definition,
        ));
    }

    let constraint_rows = sqlx::query(
        r#"
        select
          c.conname as constraint_name,
          case c.contype
            when 'p' then 'PRIMARY KEY'
            when 'f' then 'FOREIGN KEY'
            when 'u' then 'UNIQUE'
            when 'c' then 'CHECK'
            when 'x' then 'EXCLUDE'
            else c.contype::text
          end as constraint_type,
          pg_get_constraintdef(c.oid, true) as definition
        from pg_constraint c
        join pg_class t on t.oid = c.conrelid
        join pg_namespace n on n.oid = t.relnamespace
        where n.nspname = $1
          and t.relname = $2
        order by c.conname
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;
    for row in constraint_rows {
        let constraint_name = row
            .try_get::<String, _>("constraint_name")
            .map_err(DatabaseError::Postgres)?;
        let constraint_type = row
            .try_get::<String, _>("constraint_type")
            .unwrap_or_else(|_| "CONSTRAINT".to_string());
        let definition = row
            .try_get::<String, _>("definition")
            .unwrap_or_else(|_| String::new());

        rows.push(structure_row(
            "constraint",
            constraint_name,
            constraint_type,
            String::new(),
            definition,
        ));
    }

    let trigger_rows = sqlx::query(
        r#"
        select
          trigger_name,
          action_timing,
          string_agg(distinct event_manipulation, ', ' order by event_manipulation) as events,
          action_statement
        from information_schema.triggers
        where event_object_schema = $1
          and event_object_table = $2
        group by trigger_name, action_timing, action_statement
        order by trigger_name
        "#,
    )
    .bind(&schema_name)
    .bind(&table)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;
    for row in trigger_rows {
        let trigger_name = row
            .try_get::<String, _>("trigger_name")
            .map_err(DatabaseError::Postgres)?;
        let timing = row
            .try_get::<String, _>("action_timing")
            .unwrap_or_else(|_| String::new());
        let events = row
            .try_get::<String, _>("events")
            .unwrap_or_else(|_| String::new());
        let action = row
            .try_get::<String, _>("action_statement")
            .unwrap_or_else(|_| String::new());

        rows.push(structure_row(
            "trigger",
            trigger_name,
            super::join_non_empty([
                (!timing.is_empty()).then_some(timing),
                (!events.is_empty()).then_some(events),
            ]),
            String::new(),
            action,
        ));
    }

    Ok(QueryOutput::Table(structure_page(rows)))
}

/// Загружает все внешние ключи пользовательских схем PostgreSQL.
/// Разбираем `conkey`/`confkey` через `unnest ... with ordinality`,
/// чтобы получить по строке на каждую пару колонок (корректно для
/// составных FK). Системные схемы отбрасываем.
pub async fn load_foreign_keys_postgres(
    pool: &sqlx::PgPool,
) -> Result<Vec<TableForeignKey>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select
          c.conname as fk_name,
          n_from.nspname as from_schema,
          tbl_from.relname as from_table,
          a_from.attname as from_column,
          n_to.nspname as to_schema,
          tbl_to.relname as to_table,
          a_to.attname as to_column
        from pg_constraint c
          join pg_class tbl_from on tbl_from.oid = c.conrelid
          join pg_namespace n_from on n_from.oid = tbl_from.relnamespace
          join pg_class tbl_to on tbl_to.oid = c.confrelid
          join pg_namespace n_to on n_to.oid = tbl_to.relnamespace
          join lateral unnest(c.conkey) with ordinality as k(attnum, ord) on true
          join lateral unnest(c.confkey) with ordinality as fk(attnum, ord)
            on fk.ord = k.ord
          join pg_attribute a_from
            on a_from.attrelid = c.conrelid and a_from.attnum = k.attnum
          join pg_attribute a_to
            on a_to.attrelid = c.confrelid and a_to.attnum = fk.attnum
        where c.contype = 'f'
          and n_from.nspname not in ('pg_catalog', 'information_schema')
          and n_to.nspname not in ('pg_catalog', 'information_schema')
        order by n_from.nspname, tbl_from.relname, k.ord
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;

    let mut foreign_keys = Vec::new();
    for row in rows {
        foreign_keys.push(TableForeignKey {
            name: row.try_get::<String, _>("fk_name").unwrap_or_default(),
            from_schema: row.try_get::<String, _>("from_schema").unwrap_or_default(),
            from_table: row.try_get::<String, _>("from_table").unwrap_or_default(),
            from_column: row.try_get::<String, _>("from_column").unwrap_or_default(),
            to_schema: row.try_get::<String, _>("to_schema").unwrap_or_default(),
            to_table: row.try_get::<String, _>("to_table").unwrap_or_default(),
            to_column: row.try_get::<String, _>("to_column").unwrap_or_default(),
        });
    }
    Ok(foreign_keys)
}

pub async fn load_table_columns_postgres(
    pool: &sqlx::PgPool,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| "public".to_string());
    let rows = sqlx::query(
        r#"
        select column_name
        from information_schema.columns
        where table_schema = $1
          and table_name = $2
        order by ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("column_name")
                .map_err(DatabaseError::Postgres)
        })
        .collect()
}

pub async fn load_connection_tree_postgres(
    pool: &sqlx::PgPool,
) -> Result<Vec<ExplorerNode>, DatabaseError> {
    let rows = sqlx::query(
        r#"
        select table_schema, table_name, table_type
        from information_schema.tables
        where table_schema not in ('pg_catalog', 'information_schema')
        order by table_schema, table_type, table_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(DatabaseError::Postgres)?;

    let mut grouped: std::collections::BTreeMap<String, Vec<ExplorerNode>> =
        std::collections::BTreeMap::new();

    for row in rows {
        let schema = row
            .try_get::<String, _>("table_schema")
            .map_err(DatabaseError::Postgres)?;
        let name = row
            .try_get::<String, _>("table_name")
            .map_err(DatabaseError::Postgres)?;
        let table_type = row
            .try_get::<String, _>("table_type")
            .map_err(DatabaseError::Postgres)?;

        let kind = if table_type.eq_ignore_ascii_case("view") {
            ExplorerNodeKind::View
        } else {
            ExplorerNodeKind::Table
        };
        let qualified_name = format!(
            "{}.{}",
            super::quote_identifier(&schema),
            super::quote_identifier(&name)
        );

        grouped
            .entry(schema.clone())
            .or_default()
            .push(ExplorerNode {
                qualified_name,
                schema: Some(schema.clone()),
                name,
                kind,
                children: Vec::new(),
            });
    }

    Ok(grouped
        .into_iter()
        .map(|(schema, children)| ExplorerNode {
            qualified_name: super::quote_identifier(&schema),
            schema: Some(schema.clone()),
            name: schema,
            kind: ExplorerNodeKind::Schema,
            children,
        })
        .collect())
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

fn postgres_column_details(is_nullable: &str, default_value: Option<String>) -> String {
    super::join_non_empty([
        is_nullable
            .eq_ignore_ascii_case("NO")
            .then(|| "NOT NULL".to_string()),
        default_value.map(|value| format!("default {value}")),
    ])
}
