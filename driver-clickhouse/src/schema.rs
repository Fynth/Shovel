// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::{SchemaExec, quote_ident_backtick};
use models::{
    ClickHouseFormData,
    DatabaseError,
    ExplorerNode,
    ExplorerNodeKind,
    QueryOutput,
    TableForeignKey,
};

use crate::session::ClickHouseSession;

#[async_trait]
impl SchemaExec for ClickHouseSession {
    async fn describe_table(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<QueryOutput, DatabaseError> {
        describe_table(&self.config, schema, table).await
    }

    async fn load_table_columns(
        &self,
        schema: Option<String>,
        table: String,
    ) -> Result<Vec<String>, DatabaseError> {
        load_table_columns(&self.config, schema, table).await
    }

    async fn load_connection_tree(&self) -> Result<Vec<ExplorerNode>, DatabaseError> {
        load_connection_tree(&self.config).await
    }

    async fn load_foreign_keys(&self) -> Result<Vec<TableForeignKey>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn load_object_ddl(
        &self,
        schema: Option<String>,
        object: String,
        _kind: ExplorerNodeKind,
    ) -> Result<Option<String>, DatabaseError> {
        load_object_ddl(&self.config, schema, object).await
    }
}

async fn describe_table(
    config: &ClickHouseFormData,
    schema: Option<String>,
    table: String,
) -> Result<QueryOutput, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| config.database.clone());
    let mut rows = Vec::new();

    let overview_sql = format!(
        r#"
                select
                  engine,
                  partition_key,
                  sorting_key,
                  primary_key,
                  sampling_key
                from system.tables
                where database = {}
                  and name = {}
                limit 1
                "#,
        clickhouse_string_literal(&schema_name),
        clickhouse_string_literal(&table)
    );
    let overview = crate::execute_json_query(config, &overview_sql)
        .await
        .map_err(DatabaseError::Driver)?;
    if let Some(row) = overview.data.first() {
        let engine = clickhouse_value_to_string(row.first());
        let partition_key = clickhouse_value_to_string(row.get(1));
        let sorting_key = clickhouse_value_to_string(row.get(2));
        let primary_key = clickhouse_value_to_string(row.get(3));
        let sampling_key = clickhouse_value_to_string(row.get(4));

        rows.push(structure_row(
            "table",
            table.clone(),
            engine,
            String::new(),
            join_non_empty([
                meaningful_clickhouse_value(&partition_key)
                    .then(|| format!("partition key: {partition_key}")),
                meaningful_clickhouse_value(&sorting_key)
                    .then(|| format!("sorting key: {sorting_key}")),
                meaningful_clickhouse_value(&primary_key)
                    .then(|| format!("primary key: {primary_key}")),
                meaningful_clickhouse_value(&sampling_key)
                    .then(|| format!("sampling key: {sampling_key}")),
            ]),
        ));
    }

    let create_sql = if schema_name.is_empty() {
        format!("SHOW CREATE TABLE {}", quote_ident_backtick(&table))
    } else {
        format!(
            "SHOW CREATE TABLE {}.{}",
            quote_ident_backtick(&schema_name),
            quote_ident_backtick(&table)
        )
    };
    let create_statement = crate::execute_text_query(config, &create_sql)
        .await
        .map_err(DatabaseError::Driver)?;
    rows.push(structure_row(
        "table",
        table.clone(),
        "definition",
        String::new(),
        create_statement.trim().to_string(),
    ));

    let columns_sql = format!(
        r#"
                select
                  name,
                  type,
                  default_kind,
                  default_expression,
                  is_in_partition_key,
                  is_in_sorting_key,
                  is_in_primary_key
                from system.columns
                where database = {}
                  and table = {}
                order by position
                "#,
        clickhouse_string_literal(&schema_name),
        clickhouse_string_literal(&table)
    );
    let columns = crate::execute_json_query(config, &columns_sql)
        .await
        .map_err(DatabaseError::Driver)?;
    for row in columns.data {
        let column_name = clickhouse_value_to_string(row.first());
        let column_type = clickhouse_value_to_string(row.get(1));
        let default_kind = clickhouse_value_to_string(row.get(2));
        let default_expression = clickhouse_value_to_string(row.get(3));
        let in_partition_key = clickhouse_value_to_string(row.get(4)) == "1";
        let in_sorting_key = clickhouse_value_to_string(row.get(5)) == "1";
        let in_primary_key = clickhouse_value_to_string(row.get(6)) == "1";

        rows.push(structure_row(
            "column",
            column_name,
            column_type,
            String::new(),
            join_non_empty([
                meaningful_clickhouse_value(&default_kind)
                    .then(|| format!("default kind: {default_kind}")),
                meaningful_clickhouse_value(&default_expression)
                    .then(|| format!("default: {default_expression}")),
                in_partition_key.then(|| "partition key".to_string()),
                in_sorting_key.then(|| "sorting key".to_string()),
                in_primary_key.then(|| "primary key".to_string()),
            ]),
        ));
    }

    Ok(QueryOutput::Table(structure_page(rows)))
}

async fn load_table_columns(
    config: &ClickHouseFormData,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| config.database.clone());
    let sql = format!(
        "select name from system.columns where database = {} and table = {} order by position",
        clickhouse_string_literal(&schema_name),
        clickhouse_string_literal(&table)
    );
    let response = crate::execute_json_query(config, &sql)
        .await
        .map_err(DatabaseError::Driver)?;

    Ok(response
        .data
        .into_iter()
        .filter_map(|row| row.first().map(clickhouse_json_value_to_string))
        .collect())
}

async fn load_connection_tree(
    config: &ClickHouseFormData,
) -> Result<Vec<ExplorerNode>, DatabaseError> {
    let response = crate::execute_json_query(
        config,
        r#"
                select database, name, engine, create_table_query, total_rows
                from system.tables
                where database not in ('system', 'INFORMATION_SCHEMA', 'information_schema')
                order by database, name
                "#,
    )
    .await
    .map_err(DatabaseError::Driver)?;

    let mut grouped: std::collections::BTreeMap<String, Vec<ExplorerNode>> =
        std::collections::BTreeMap::new();

    for row in response.data {
        let schema = clickhouse_value_to_string(row.first());
        let name = clickhouse_value_to_string(row.get(1));
        let engine = clickhouse_value_to_string(row.get(2));
        let create_table_query = clickhouse_value_to_string(row.get(3));
        let total_rows = row.get(4).and_then(|value| value.as_u64());
        if !clickhouse_relation_supports_preview(&engine, &create_table_query) {
            continue;
        }
        let kind = if engine.to_ascii_lowercase().contains("view") {
            ExplorerNodeKind::View
        } else {
            ExplorerNodeKind::Table
        };
        let qualified_name = format!(
            "{}.{}",
            quote_ident_backtick(&schema),
            quote_ident_backtick(&name)
        );

        grouped
            .entry(schema.clone())
            .or_default()
            .push(ExplorerNode {
                qualified_name,
                schema: Some(schema.clone()),
                name,
                kind,
                row_count: match kind {
                    ExplorerNodeKind::Table => total_rows.filter(|count| *count > 0),
                    _ => None,
                },
                children: Vec::new(),
            });
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

async fn load_object_ddl(
    config: &ClickHouseFormData,
    schema: Option<String>,
    object: String,
) -> Result<Option<String>, DatabaseError> {
    let schema_name = schema.unwrap_or_else(|| config.database.clone());
    let sql = format!(
        "select create_table_query from system.tables where database = {} and name = {}",
        clickhouse_string_literal(&schema_name),
        clickhouse_string_literal(&object)
    );
    let response = crate::execute_json_query(config, &sql)
        .await
        .map_err(DatabaseError::Driver)?;
    Ok(response
        .data
        .into_iter()
        .next()
        .and_then(|row| row.first().map(clickhouse_json_value_to_string))
        .filter(|s| !s.trim().is_empty()))
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

fn clickhouse_relation_supports_preview(engine: &str, create_table_query: &str) -> bool {
    let normalized_engine = engine.trim().to_ascii_lowercase();
    if matches!(
        normalized_engine.as_str(),
        "kafka" | "rabbitmq" | "nats" | "s3queue" | "azurequeue" | "redis"
    ) {
        return false;
    }

    if normalized_engine == "materializedview" {
        return !clickhouse_materialized_view_targets_table(create_table_query);
    }

    true
}

fn clickhouse_materialized_view_targets_table(create_table_query: &str) -> bool {
    let normalized = create_table_query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    if !normalized.starts_with("CREATE MATERIALIZED VIEW ") {
        return false;
    }

    let definition_head = normalized
        .split_once(" AS ")
        .map(|(head, _)| head)
        .unwrap_or(&normalized);
    definition_head.contains(" TO ")
}

fn meaningful_clickhouse_value(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed != "NULL"
}

fn join_non_empty(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn clickhouse_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn clickhouse_value_to_string(value: Option<&serde_json::Value>) -> String {
    value
        .map(clickhouse_json_value_to_string)
        .unwrap_or_else(|| "NULL".to_string())
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

#[cfg(test)]
mod tests {
    use super::{clickhouse_materialized_view_targets_table, clickhouse_relation_supports_preview};

    #[test]
    fn hides_stream_like_clickhouse_engines_from_preview_tree() {
        assert!(!clickhouse_relation_supports_preview(
            "Kafka",
            "CREATE TABLE dwh_ogs.source_statistics_kafka ENGINE = Kafka"
        ));
        assert!(!clickhouse_relation_supports_preview(
            "RabbitMQ",
            "CREATE TABLE dwh_ogs.source_statistics_queue ENGINE = RabbitMQ"
        ));
    }

    #[test]
    fn hides_materialized_views_with_to_target_from_preview_tree() {
        assert!(clickhouse_materialized_view_targets_table(
            "CREATE MATERIALIZED VIEW dwh_ogs.mv TO dwh_ogs.target AS SELECT * FROM dwh_ogs.src"
        ));
        assert!(!clickhouse_relation_supports_preview(
            "MaterializedView",
            "CREATE MATERIALIZED VIEW dwh_ogs.mv TO dwh_ogs.target AS SELECT * FROM dwh_ogs.src"
        ));
    }

    #[test]
    fn keeps_regular_clickhouse_views_and_tables_previewable() {
        assert!(clickhouse_relation_supports_preview(
            "MergeTree",
            "CREATE TABLE dwh_ogs.source_statistics ENGINE = MergeTree ORDER BY tuple()"
        ));
        assert!(clickhouse_relation_supports_preview(
            "View",
            "CREATE VIEW dwh_ogs.source_statistics_view AS SELECT * FROM dwh_ogs.source_statistics"
        ));
        assert!(clickhouse_relation_supports_preview(
            "MaterializedView",
            "CREATE MATERIALIZED VIEW dwh_ogs.mv ENGINE = MergeTree ORDER BY tuple() AS SELECT 1"
        ));
    }
}
