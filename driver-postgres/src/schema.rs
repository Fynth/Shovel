// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{SchemaExec, quote_ident_double};
use models::{DatabaseError, ExplorerNode, ExplorerNodeKind, QueryOutput, TableForeignKey};
use sqlx::Row;

use crate::session::PostgresSession;

#[async_trait]
impl SchemaExec for PostgresSession {
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in column_rows {
        let column_name = row
            .try_get::<String, _>("column_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in index_rows {
        let index_name = row
            .try_get::<String, _>("indexname")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in constraint_rows {
        let constraint_name = row
            .try_get::<String, _>("constraint_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in trigger_rows {
        let trigger_name = row
            .try_get::<String, _>("trigger_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
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
            join_non_empty([
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
async fn load_foreign_keys(pool: &sqlx::PgPool) -> Result<Vec<TableForeignKey>, DatabaseError> {
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

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

/// Возвращает DDL объекта PostgreSQL. Для представлений и MV —
/// `pg_get_viewdef` (точный текст). Для таблиц — реконструкция из
/// `information_schema.columns`, `pg_constraint` и `pg_indexes`, т.к. у PG
/// нет встроенного `SHOW CREATE`. Для последовательностей, функций,
/// процедур и триггеров — соответствующие `pg_get_*def`. Возвращает
/// `None`, если объект не найден.
async fn load_object_ddl(
    pool: &sqlx::PgPool,
    schema: Option<String>,
    object: String,
    kind: ExplorerNodeKind,
) -> Result<Option<String>, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| "public".to_string());

    match kind {
        ExplorerNodeKind::View | ExplorerNodeKind::MaterializedView => {
            let ddl = sqlx::query_scalar::<_, Option<String>>(
                r#"
                select pg_get_viewdef(c.oid, true)
                from pg_class c
                join pg_namespace n on n.oid = c.relnamespace
                where n.nspname = $1 and c.relname = $2
                "#,
            )
            .bind(&schema_name)
            .bind(&object)
            .fetch_optional(pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?
            .flatten();
            Ok(ddl.filter(|s| !s.trim().is_empty()))
        }
        ExplorerNodeKind::Sequence => {
            let ddl = sqlx::query_scalar::<_, Option<String>>(
                r#"
                select pg_get_sequencedef(c.oid)
                from pg_class c
                join pg_namespace n on n.oid = c.relnamespace
                where n.nspname = $1 and c.relname = $2
                "#,
            )
            .bind(&schema_name)
            .bind(&object)
            .fetch_optional(pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?
            .flatten();
            Ok(ddl.filter(|s| !s.trim().is_empty()))
        }
        ExplorerNodeKind::Function | ExplorerNodeKind::Procedure => {
            // pg_get_functiondef работает и для функций, и для процедур.
            // Если несколько перегрузок с одним именем — берём первую.
            let ddl = sqlx::query_scalar::<_, Option<String>>(
                r#"
                select pg_get_functiondef(p.oid)
                from pg_proc p
                join pg_namespace n on n.oid = p.pronamespace
                where n.nspname = $1 and p.proname = $2
                order by p.oid
                limit 1
                "#,
            )
            .bind(&schema_name)
            .bind(&object)
            .fetch_optional(pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?
            .flatten();
            Ok(ddl.filter(|s| !s.trim().is_empty()))
        }
        ExplorerNodeKind::Trigger => {
            let ddl = sqlx::query_scalar::<_, Option<String>>(
                r#"
                select pg_get_triggerdef(t.oid)
                from pg_trigger t
                join pg_class c on c.oid = t.tgrelid
                join pg_namespace n on n.oid = c.relnamespace
                where n.nspname = $1 and t.tgname = $2
                "#,
            )
            .bind(&schema_name)
            .bind(&object)
            .fetch_optional(pool)
            .await
            .map_err(|e| DatabaseError::Driver(e.to_string()))?
            .flatten();
            Ok(ddl.filter(|s| !s.trim().is_empty()))
        }
        ExplorerNodeKind::Table | ExplorerNodeKind::Schema | ExplorerNodeKind::Column =>
            reconstruct_postgres_table_ddl(pool, &schema_name, &object)
                .await
                .map(Some),
    }
}

/// Реконструирует `CREATE TABLE` для PostgreSQL: колонки, первичный ключ,
/// внешние ключи, уникальные и check-ограничения (через `pg_get_constraintdef`)
/// и неуникальные индексы отдельными операторами (`pg_get_indexdef`).
/// Уникальные/первичные индексы не дублируются — они покрыты ограничениями.
async fn reconstruct_postgres_table_ddl(
    pool: &sqlx::PgPool,
    schema_name: &str,
    table: &str,
) -> Result<String, DatabaseError> {
    let qualified = format!(
        "{}.{}",
        quote_ident_double(schema_name),
        quote_ident_double(table)
    );

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("CREATE TABLE {qualified} ("));

    // Колонки
    let column_rows = sqlx::query(
        r#"
        select column_name, data_type, is_nullable, column_default
        from information_schema.columns
        where table_schema = $1 and table_name = $2
        order by ordinal_position
        "#,
    )
    .bind(schema_name)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut column_lines: Vec<String> = Vec::new();
    for row in column_rows {
        let name = row
            .try_get::<String, _>("column_name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let data_type = row
            .try_get::<String, _>("data_type")
            .unwrap_or_else(|_| "text".to_string());
        let is_nullable = row
            .try_get::<String, _>("is_nullable")
            .unwrap_or_else(|_| "YES".to_string());
        let default = row
            .try_get::<Option<String>, _>("column_default")
            .ok()
            .flatten();

        let mut col = format!("    {} {}", quote_ident_double(&name), data_type);
        if is_nullable.eq_ignore_ascii_case("NO") {
            col.push_str(" NOT NULL");
        }
        if let Some(default) = default {
            col.push_str(&format!(" DEFAULT {default}"));
        }
        column_lines.push(col);
    }

    // Ограничения (PK / FK / UNIQUE / CHECK) — текст из pg_get_constraintdef
    let constraint_rows = sqlx::query(
        r#"
        select pg_get_constraintdef(c.oid, true) as definition
        from pg_constraint c
        join pg_class t on t.oid = c.conrelid
        join pg_namespace n on n.oid = t.relnamespace
        where n.nspname = $1 and t.relname = $2
        order by c.contype, c.conname
        "#,
    )
    .bind(schema_name)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in constraint_rows {
        if let Some(def) = row
            .try_get::<Option<String>, _>("definition")
            .ok()
            .flatten()
            .filter(|d| !d.trim().is_empty())
        {
            column_lines.push(format!("    {def}"));
        }
    }

    lines.push(column_lines.join(",\n"));
    lines.push(");".to_string());

    // Неуникальные индексы отдельными операторами.
    let index_rows = sqlx::query(
        r#"
        select pg_get_indexdef(i.indexrelid) as indexdef
        from pg_index i
        join pg_class c on c.oid = i.indrelid
        join pg_namespace n on n.oid = c.relnamespace
        where n.nspname = $1 and c.relname = $2
          and not i.indisunique and not i.indisprimary
        "#,
    )
    .bind(schema_name)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in index_rows {
        if let Some(def) = row
            .try_get::<Option<String>, _>("indexdef")
            .ok()
            .flatten()
            .filter(|d| !d.trim().is_empty())
        {
            lines.push(String::new());
            lines.push(format!("{def};"));
        }
    }

    Ok(lines.join("\n"))
}

async fn load_table_columns(
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    rows.into_iter()
        .map(|row| {
            row.try_get::<String, _>("column_name")
                .map_err(|e| DatabaseError::Driver(e.to_string()))
        })
        .collect()
}

async fn load_connection_tree(pool: &sqlx::PgPool) -> Result<Vec<ExplorerNode>, DatabaseError> {
    let mut grouped: std::collections::BTreeMap<String, Vec<ExplorerNode>> =
        std::collections::BTreeMap::new();

    let push_node = |grouped: &mut std::collections::BTreeMap<String, Vec<ExplorerNode>>,
                     schema: String,
                     name: String,
                     kind: ExplorerNodeKind,
                     row_count: Option<u64>| {
        let qualified_name = format!(
            "{}.{}",
            quote_ident_double(&schema),
            quote_ident_double(&name)
        );
        grouped
            .entry(schema.clone())
            .or_default()
            .push(ExplorerNode {
                qualified_name,
                schema: Some(schema),
                name,
                kind,
                row_count,
                children: Vec::new(),
            });
    };

    // Таблицы и представления.
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
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    // Cheap per-relation row estimates from pg_class.reltuples (statistics,
    // NOT a full COUNT). Keyed by (schema, name).
    let reltuples = load_pg_row_estimates(pool).await;

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
            ExplorerNodeKind::Table => reltuples.get(&(schema.clone(), name.clone())).copied(),
            _ => None,
        };
        push_node(&mut grouped, schema, name, kind, row_count);
    }

    // Материализованные представления и последовательности (relkind).
    let rows = sqlx::query(
        r#"
        select n.nspname as schema, c.relname as name, c.relkind, c.reltuples::bigint as reltuples
        from pg_class c
        join pg_namespace n on n.oid = c.relnamespace
        where n.nspname not in ('pg_catalog', 'information_schema')
          and c.relkind in ('m', 'S')
        order by n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let relkind = row.try_get::<String, _>("relkind").unwrap_or_default();
        let reltuples = row.try_get::<i64, _>("reltuples").ok();
        let kind = match relkind.as_str() {
            "m" => ExplorerNodeKind::MaterializedView,
            "S" => ExplorerNodeKind::Sequence,
            _ => continue,
        };
        let row_count = match kind {
            ExplorerNodeKind::MaterializedView => reltuples_to_count(reltuples),
            _ => None,
        };
        push_node(&mut grouped, schema, name, kind, row_count);
    }

    // Функции и процедуры (pg_proc, prokind; отбрасываем агрегаты и
    // встроенные пакеты). prokind: 'f'=function, 'p'=procedure.
    let rows = sqlx::query(
        r#"
        select n.nspname as schema, p.proname as name, p.prokind
        from pg_proc p
        join pg_namespace n on n.oid = p.pronamespace
        where n.nspname not in ('pg_catalog', 'information_schema')
          and p.prokind in ('f', 'p')
        order by n.nspname, p.proname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let prokind = row.try_get::<String, _>("prokind").unwrap_or_default();
        let kind = match prokind.as_str() {
            "p" => ExplorerNodeKind::Procedure,
            _ => ExplorerNodeKind::Function,
        };
        push_node(&mut grouped, schema, name, kind, None);
    }

    // Триггеры (отбрасываем внутренние trigger-функции pg_trigger.tgisinternal).
    let rows = sqlx::query(
        r#"
        select n.nspname as schema, t.tgname as name
        from pg_trigger t
        join pg_class c on c.oid = t.tgrelid
        join pg_namespace n on n.oid = c.relnamespace
        where n.nspname not in ('pg_catalog', 'information_schema')
          and not t.tgisinternal
        order by n.nspname, t.tgname
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| DatabaseError::Driver(e.to_string()))?;
    for row in rows {
        let schema = row
            .try_get::<String, _>("schema")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        let name = row
            .try_get::<String, _>("name")
            .map_err(|e| DatabaseError::Driver(e.to_string()))?;
        push_node(&mut grouped, schema, name, ExplorerNodeKind::Trigger, None);
    }

    Ok(grouped
        .into_iter()
        .map(|(schema, children)| ExplorerNode {
            qualified_name: quote_ident_double(&schema),
            schema: Some(schema.clone()),
            name: schema,
            kind: ExplorerNodeKind::Schema,
            row_count: None,
            children,
        })
        .collect())
}

/// Загружает оценки числа строк для всех обычных таблиц пользовательских
/// схем из `pg_class.reltuples` (статистика планировщика, дёшево — без
/// блокирующего `COUNT(*)`). Возвращает `HashMap<(schema, name), rows>`.
/// Любая ошибка запроса → пустая карта (никогда не ломает дерево).
async fn load_pg_row_estimates(
    pool: &sqlx::PgPool,
) -> std::collections::HashMap<(String, String), u64> {
    let Ok(rows) = sqlx::query(
        r#"
        select n.nspname as schema, c.relname as name, c.reltuples::bigint as reltuples
        from pg_class c
        join pg_namespace n on n.oid = c.relnamespace
        where n.nspname not in ('pg_catalog', 'information_schema')
          and c.relkind = 'r'
        order by n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await
    else {
        return std::collections::HashMap::new();
    };

    rows.into_iter()
        .filter_map(|row| {
            let schema = row.try_get::<String, _>("schema").ok()?;
            let name = row.try_get::<String, _>("name").ok()?;
            let count = reltuples_to_count(row.try_get::<i64, _>("reltuples").ok())?;
            Some(((schema, name), count))
        })
        .collect()
}

/// Преобразует сырое значение `pg_class.reltuples` в неотрицательную оценку
/// числа строк. Отрицательные значения (статистика ещё не собрана) и ошибки
/// типов дают `None`.
fn reltuples_to_count(reltuples: Option<i64>) -> Option<u64> {
    reltuples
        .filter(|count| *count >= 0)
        .and_then(|count| u64::try_from(count).ok())
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

fn join_non_empty(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn postgres_column_details(is_nullable: &str, default_value: Option<String>) -> String {
    join_non_empty([
        is_nullable
            .eq_ignore_ascii_case("NO")
            .then(|| "NOT NULL".to_string()),
        default_value.map(|value| format!("default {value}")),
    ])
}

#[cfg(test)]
mod tests {
    use super::reltuples_to_count;

    #[test]
    fn reltuples_negative_or_unknown_yields_none() {
        assert_eq!(reltuples_to_count(None), None);
        assert_eq!(reltuples_to_count(Some(-1)), None);
    }

    #[test]
    fn reltuples_non_negative_becomes_row_count() {
        assert_eq!(reltuples_to_count(Some(0)), Some(0));
        assert_eq!(reltuples_to_count(Some(42)), Some(42));
    }
}
