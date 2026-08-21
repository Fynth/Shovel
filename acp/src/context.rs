use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use models::{
    DatabaseConnection, DatabaseError, ExplorerNode, ExplorerNodeKind, QueryHistoryItem,
    QueryOutput, QueryPage, TablePreviewSource,
};

use explorer::{describe_table, load_connection_tree};
use query::load_table_preview_page;

const MAX_PREVIEW_CONTEXT_TABLES: usize = 1;
const MAX_CONTEXT_ROWS: usize = 3;
const MAX_CONTEXT_COLUMNS: usize = 8;
const MAX_CONTEXT_META_ITEMS: usize = 6;
const MAX_OBSERVED_VALUE_COLUMNS: usize = 5;
const MAX_OBSERVED_VALUES_PER_COLUMN: usize = 3;
const MAX_INLINE_VALUE_LEN: usize = 48;
const MAX_INLINE_DETAILS_LEN: usize = 240;
const SCHEMA_CACHE_TTL: Duration = Duration::from_secs(90);
const FULL_CONTEXT_CACHE_TTL: Duration = Duration::from_secs(90);
const MAX_HISTORY_QUERIES: usize = 5;
const MAX_HISTORY_QUERY_LENGTH: usize = 200;
const CONTEXT_TOKEN_BUDGET: usize = 4000;
const AVG_CHARS_PER_TOKEN: usize = 4;

struct CachedSchemaContext {
    catalog_signature: String,
    built_at: Instant,
    lines: Vec<String>,
}

struct CachedFullContext {
    connection_label: String,
    built_at: Instant,
    context: String,
}

fn schema_context_cache() -> &'static Mutex<HashMap<String, CachedSchemaContext>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedSchemaContext>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn full_context_cache() -> &'static Mutex<HashMap<String, CachedFullContext>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedFullContext>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Check if a query contains sensitive information that should be redacted.
fn contains_sensitive_info(query: &str) -> bool {
    let upper = query.to_uppercase();
    upper.contains("PASSWORD")
        || upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("KEY")
        || upper.contains("CREDENTIAL")
        || upper.contains("API_KEY")
        || upper.contains("AUTH")
}

/// Redact sensitive information from a query.
fn redact_sensitive_query(query: &str) -> String {
    if contains_sensitive_info(query) {
        "-- [REDACTED: contains sensitive information]".to_string()
    } else {
        query.to_string()
    }
}

/// Estimate token count from character count.
fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / AVG_CHARS_PER_TOKEN
}

/// Check if context exceeds token budget and log warning.
fn check_token_budget(context: &str, source: &str) {
    let tokens = estimate_tokens(context);
    if tokens > CONTEXT_TOKEN_BUDGET {
        tracing::warn!(
            "ACP context from '{}' exceeds token budget: {} tokens (limit: {})",
            source,
            tokens,
            CONTEXT_TOKEN_BUDGET
        );
    }
}

/// Format query history items for AI context.
fn format_query_history_item(item: &QueryHistoryItem, index: usize) -> String {
    let sql = redact_sensitive_query(&item.sql);
    let sql_preview = if sql.len() > MAX_HISTORY_QUERY_LENGTH {
        format!("{}...", &sql[..MAX_HISTORY_QUERY_LENGTH])
    } else {
        sql
    };

    let rows_info = item
        .rows_returned
        .map(|r| format!(", {} rows", r))
        .unwrap_or_default();

    format!(
        "  {}. [{}ms{}] {}",
        index + 1,
        item.duration_ms,
        rows_info,
        sql_preview.replace('\n', " ")
    )
}

/// Append execution history to context lines.
async fn append_execution_history(lines: &mut Vec<String>) {
    match storage::QueryHistoryStore::load(MAX_HISTORY_QUERIES).await {
        Ok(history) => {
            if history.is_empty() {
                return;
            }

            lines.push(String::new());
            lines.push(format!(
                "Recent query execution history (last {} queries):",
                history.len().min(MAX_HISTORY_QUERIES)
            ));

            for (index, item) in history.iter().take(MAX_HISTORY_QUERIES).enumerate() {
                lines.push(format_query_history_item(item, index));
            }
        }
        Err(err) => {
            tracing::debug!("Failed to load query history for context: {}", err);
        }
    }
}

/// Append performance metrics placeholder.
/// For full introspection metrics, use `build_full_ai_context` with introspection data.
async fn append_performance_metrics(_lines: &mut Vec<String>, _connection: &DatabaseConnection) {
    // This is a placeholder - actual introspection metrics are added via
    // `append_introspection_metrics` when calling `build_full_ai_context`
}

/// Append performance metrics from pre-collected introspection data.
pub fn append_introspection_metrics(
    lines: &mut Vec<String>,
    introspection: &crate::introspection::IntrospectionResult,
) {
    // Add slowest queries
    if !introspection.query_history.is_empty() {
        lines.push(String::new());
        lines.push("Slowest queries (from pg_stat_statements):".to_string());

        for (index, entry) in introspection.query_history.iter().take(3).enumerate() {
            let query_preview = if entry.query.len() > MAX_HISTORY_QUERY_LENGTH {
                format!("{}...", &entry.query[..MAX_HISTORY_QUERY_LENGTH])
            } else {
                entry.query.clone()
            };
            let redacted = redact_sensitive_query(&query_preview);

            lines.push(format!(
                "  {}. [{}ms avg, {} calls, {} rows] {}",
                index + 1,
                entry.mean_time_ms as i64,
                entry.calls,
                entry.rows,
                redacted.replace('\n', " ")
            ));
        }
    }

    // Add active locks
    if !introspection.locks.is_empty() {
        lines.push(String::new());
        lines.push("Active locks:".to_string());

        for lock in introspection.locks.iter().take(5) {
            let relation = lock.relation.as_deref().unwrap_or("unknown");
            let status = if lock.granted { "granted" } else { "waiting" };
            lines.push(format!("  - {} on {} ({})", lock.mode, relation, status));
        }

        if introspection.locks.len() > 5 {
            lines.push(format!(
                "  ... {} more locks",
                introspection.locks.len() - 5
            ));
        }
    }

    // Add index usage summary
    if !introspection.index_stats.is_empty() {
        lines.push(String::new());
        lines.push("Index usage (top by scans):".to_string());

        for stat in introspection.index_stats.iter().take(3) {
            lines.push(format!(
                "  - {}.{}: {} scans",
                stat.schema, stat.index_name, stat.idx_scan
            ));
        }
    }

    // Add connection pool stats from active queries
    if !introspection.active_queries.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Active connections: {} (non-idle)",
            introspection.active_queries.len()
        ));

        for query in introspection.active_queries.iter().take(3) {
            let duration = query
                .duration_ms
                .map(|d| format!("{}ms", d))
                .unwrap_or_else(|| "unknown".to_string());
            let query_preview = if query.query.len() > 60 {
                format!("{}...", &query.query[..60])
            } else {
                query.query.clone()
            };
            lines.push(format!(
                "  - [{}] {}: {}",
                duration,
                query.state,
                query_preview.replace('\n', " ")
            ));
        }
    }
}

pub async fn build_acp_database_context(
    connection: DatabaseConnection,
    connection_label: String,
    focus_source: Option<TablePreviewSource>,
) -> Result<String, DatabaseError> {
    let tree = load_connection_tree(connection.clone()).await?;
    let all_sources = collect_table_sources(&tree);
    let prioritized_sources = prioritize_table_sources(all_sources, focus_source.clone());
    let preview_sources = prioritized_sources
        .iter()
        .take(MAX_PREVIEW_CONTEXT_TABLES)
        .cloned()
        .collect::<Vec<_>>();

    let mut lines = vec![format!("Active database connection: {connection_label}")];
    append_catalog_summary(&mut lines, &tree);
    if let Some(focus_source) = focus_source.as_ref() {
        lines.push(format!(
            "Active focus relation: {}",
            focus_source.qualified_name
        ));
    }

    let schema_lines = load_or_build_schema_context_lines(
        connection.clone(),
        &connection_label,
        &tree,
        &prioritized_sources,
    )
    .await?;
    if !schema_lines.is_empty() {
        lines.push(String::new());
        lines.extend(schema_lines);
    }

    if !preview_sources.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Focused data previews (up to {MAX_PREVIEW_CONTEXT_TABLES} relation(s); preview rows are never the full table):"
        ));
    }

    for (index, source) in preview_sources.into_iter().enumerate() {
        let _ = index;
        let is_active_focus = focus_source
            .as_ref()
            .is_some_and(|focus| same_source(focus, &source));
        append_relation_preview_profile(&mut lines, connection.clone(), source, is_active_focus)
            .await;
    }

    // Add execution history
    append_execution_history(&mut lines).await;

    // Add performance metrics placeholder
    append_performance_metrics(&mut lines, &connection).await;

    let context = lines.join("\n");
    check_token_budget(&context, "build_acp_database_context");

    Ok(context)
}

/// Build full AI context combining schema, history, metrics, and introspection data.
/// This function includes caching with 90s TTL.
pub async fn build_full_ai_context(
    connection: DatabaseConnection,
    connection_label: String,
    focus_source: Option<TablePreviewSource>,
    introspection: Option<&crate::introspection::IntrospectionResult>,
) -> Result<String, DatabaseError> {
    let cache_key = format!("{}:{:?}", connection_label, focus_source);

    // Check cache first
    if let Ok(cache) = full_context_cache().lock()
        && let Some(cached) = cache.get(&cache_key)
        && cached.connection_label == connection_label
        && cached.built_at.elapsed() <= FULL_CONTEXT_CACHE_TTL
    {
        tracing::debug!("Using cached full AI context for {}", connection_label);
        return Ok(cached.context.clone());
    }

    // Build base context
    let mut lines = vec![format!("Active database connection: {connection_label}")];

    let tree = load_connection_tree(connection.clone()).await?;
    append_catalog_summary(&mut lines, &tree);

    if let Some(focus_source) = focus_source.as_ref() {
        lines.push(format!(
            "Active focus relation: {}",
            focus_source.qualified_name
        ));
    }

    // Add schema
    let all_sources = collect_table_sources(&tree);
    let prioritized_sources = prioritize_table_sources(all_sources, focus_source.clone());
    let schema_lines = load_or_build_schema_context_lines(
        connection.clone(),
        &connection_label,
        &tree,
        &prioritized_sources,
    )
    .await?;
    if !schema_lines.is_empty() {
        lines.push(String::new());
        lines.extend(schema_lines);
    }

    // Add previews
    let preview_sources = prioritized_sources
        .iter()
        .take(MAX_PREVIEW_CONTEXT_TABLES)
        .cloned()
        .collect::<Vec<_>>();

    if !preview_sources.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Focused data previews (up to {MAX_PREVIEW_CONTEXT_TABLES} relation(s):"
        ));
    }

    for source in preview_sources {
        let is_active_focus = focus_source
            .as_ref()
            .is_some_and(|focus| same_source(focus, &source));
        append_relation_preview_profile(&mut lines, connection.clone(), source, is_active_focus)
            .await;
    }

    // Add execution history
    append_execution_history(&mut lines).await;

    // Add introspection metrics if available
    if let Some(introspection_data) = introspection {
        append_introspection_metrics(&mut lines, introspection_data);
    }

    let context = lines.join("\n");
    check_token_budget(&context, "build_full_ai_context");

    // Cache the result
    if let Ok(mut cache) = full_context_cache().lock() {
        cache.insert(
            cache_key,
            CachedFullContext {
                connection_label: connection_label.clone(),
                built_at: Instant::now(),
                context: context.clone(),
            },
        );
    }

    Ok(context)
}

pub async fn warm_acp_database_schema_context(
    connection: DatabaseConnection,
    connection_label: String,
) -> Result<(), DatabaseError> {
    let tree = load_connection_tree(connection.clone()).await?;
    let sources = collect_table_sources(&tree);
    let _ =
        load_or_build_schema_context_lines(connection, &connection_label, &tree, &sources).await?;
    Ok(())
}

async fn load_or_build_schema_context_lines(
    connection: DatabaseConnection,
    connection_label: &str,
    tree: &[ExplorerNode],
    sources: &[TablePreviewSource],
) -> Result<Vec<String>, DatabaseError> {
    let catalog_signature = build_catalog_signature(tree);

    if let Ok(cache) = schema_context_cache().lock()
        && let Some(cached) = cache.get(connection_label)
        && cached.catalog_signature == catalog_signature
        && cached.built_at.elapsed() <= SCHEMA_CACHE_TTL
    {
        return Ok(cached.lines.clone());
    }

    let mut lines = Vec::new();
    if !sources.is_empty() {
        lines.push(format!(
            "Full relation schema map (all {} relation(s) currently visible in the database catalog):",
            sources.len()
        ));
    }

    for source in sources {
        append_relation_schema_profile(&mut lines, connection.clone(), source.clone()).await;
    }

    if let Ok(mut cache) = schema_context_cache().lock() {
        cache.insert(
            connection_label.to_string(),
            CachedSchemaContext {
                catalog_signature,
                built_at: Instant::now(),
                lines: lines.clone(),
            },
        );
    }

    Ok(lines)
}

async fn append_relation_schema_profile(
    lines: &mut Vec<String>,
    connection: DatabaseConnection,
    source: TablePreviewSource,
) {
    lines.push(format!("- {}", source.qualified_name));

    match describe_table(
        connection.clone(),
        source.schema.clone(),
        source.table_name.clone(),
    )
    .await
    {
        Ok(QueryOutput::Table(page)) => append_structure_profile(lines, &page, true),
        Ok(QueryOutput::AffectedRows(_)) => {
            lines.push("  structure: <non-tabular response>".to_string());
        }
        Err(err) => {
            lines.push(format!("  structure error: {err:?}"));
        }
    }
}

async fn append_relation_preview_profile(
    lines: &mut Vec<String>,
    connection: DatabaseConnection,
    source: TablePreviewSource,
    is_active_focus: bool,
) {
    lines.push(relation_heading(&source, is_active_focus));

    match load_table_preview_page(connection, source, MAX_CONTEXT_ROWS as u32, 0, None, None).await
    {
        Ok(QueryOutput::Table(page)) => {
            append_page_preview(lines, &page);
            append_observed_values(lines, &page);
        }
        Ok(QueryOutput::AffectedRows(_)) => {
            lines.push("  preview: <non-tabular response>".to_string());
        }
        Err(err) => {
            lines.push(format!("  preview error: {err:?}"));
        }
    }
}

fn relation_heading(source: &TablePreviewSource, is_active_focus: bool) -> String {
    if is_active_focus {
        format!("- {} [active focus]", source.qualified_name)
    } else {
        format!("- {}", source.qualified_name)
    }
}

fn build_catalog_signature(nodes: &[ExplorerNode]) -> String {
    let mut signature = String::new();
    append_catalog_signature_parts(nodes, &mut signature);
    signature
}

fn append_catalog_signature_parts(nodes: &[ExplorerNode], signature: &mut String) {
    for node in nodes {
        signature.push_str(match node.kind {
            ExplorerNodeKind::Schema => "schema:",
            ExplorerNodeKind::Table => "table:",
            ExplorerNodeKind::View => "view:",
        });
        signature.push_str(&node.qualified_name);
        signature.push('|');
        append_catalog_signature_parts(&node.children, signature);
    }
}

fn append_structure_profile(lines: &mut Vec<String>, page: &QueryPage, include_all_items: bool) {
    let mut definition = None::<String>;
    let mut table_meta = Vec::new();
    let mut columns = Vec::new();
    let mut other_meta = Vec::new();

    for row in &page.rows {
        let section = row.first().map(String::as_str).unwrap_or_default();
        let name = row.get(1).map(String::as_str).unwrap_or_default();
        let row_type = row.get(2).map(String::as_str).unwrap_or_default();
        let target = row.get(3).map(String::as_str).unwrap_or_default();
        let details = row.get(4).map(String::as_str).unwrap_or_default();

        match section {
            "table" if row_type.eq_ignore_ascii_case("definition") => {
                definition = Some(inline_excerpt(details, MAX_INLINE_DETAILS_LEN));
            }
            "table" => {
                table_meta.push(format_structure_item(name, row_type, target, details));
            }
            "column" => {
                columns.push(format_structure_item(name, row_type, target, details));
            }
            _ => {
                other_meta.push(format!(
                    "{section} {}",
                    format_structure_item(name, row_type, target, details)
                ));
            }
        }
    }

    if let Some(definition) = definition {
        lines.push(format!("  definition: {definition}"));
    }

    if !table_meta.is_empty() {
        lines.push("  relation details:".to_string());
        append_limited_items(
            lines,
            &table_meta,
            item_limit(table_meta.len(), include_all_items, MAX_CONTEXT_META_ITEMS),
        );
    }

    if !columns.is_empty() {
        lines.push("  columns:".to_string());
        append_limited_items(
            lines,
            &columns,
            item_limit(columns.len(), include_all_items, MAX_CONTEXT_COLUMNS),
        );
    }

    if !other_meta.is_empty() {
        lines.push("  schema details:".to_string());
        append_limited_items(
            lines,
            &other_meta,
            item_limit(other_meta.len(), include_all_items, MAX_CONTEXT_META_ITEMS),
        );
    }
}

fn append_page_preview(lines: &mut Vec<String>, page: &QueryPage) {
    if page.columns.is_empty() {
        lines.push("  columns: <none>".to_string());
        return;
    }

    lines.push(format!("  preview columns: {}", page.columns.join(", ")));
    if page.rows.is_empty() {
        lines.push("  preview: <empty>".to_string());
        return;
    }

    if page.has_next || page.offset > 0 {
        lines.push(format!(
            "  preview: showing {} row(s) from offset {} only; do not treat this as the full table",
            page.rows.len(),
            page.offset
        ));
    } else {
        lines.push(format!(
            "  preview: showing {} row(s); totals are unknown unless counted explicitly",
            page.rows.len()
        ));
    }

    for row in page.rows.iter().take(MAX_CONTEXT_ROWS) {
        let cells = page
            .columns
            .iter()
            .zip(row.iter())
            .map(|(column, value)| {
                format!("{column}={}", inline_excerpt(value, MAX_INLINE_VALUE_LEN))
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("  row: {cells}"));
    }
}

fn append_observed_values(lines: &mut Vec<String>, page: &QueryPage) {
    if page.columns.is_empty() || page.rows.is_empty() {
        return;
    }

    let mut observed = Vec::new();
    for (column_index, column_name) in page
        .columns
        .iter()
        .enumerate()
        .take(MAX_OBSERVED_VALUE_COLUMNS)
    {
        let mut values = Vec::<String>::new();
        for row in page.rows.iter().take(MAX_CONTEXT_ROWS) {
            let Some(value) = row.get(column_index) else {
                continue;
            };
            let excerpt = inline_excerpt(value, MAX_INLINE_VALUE_LEN);
            if excerpt.is_empty() || values.iter().any(|existing| existing == &excerpt) {
                continue;
            }
            values.push(excerpt);
            if values.len() >= MAX_OBSERVED_VALUES_PER_COLUMN {
                break;
            }
        }
        if !values.is_empty() {
            observed.push(format!("{column_name} = {}", values.join(" | ")));
        }
    }

    if observed.is_empty() {
        return;
    }

    lines.push("  observed values:".to_string());
    append_limited_items(lines, &observed, MAX_OBSERVED_VALUE_COLUMNS);
}

fn append_catalog_summary(lines: &mut Vec<String>, nodes: &[ExplorerNode]) {
    let schema_count = nodes
        .iter()
        .filter(|node| matches!(node.kind, ExplorerNodeKind::Schema))
        .count();
    let relation_count = nodes.iter().map(count_relations).sum::<usize>();
    lines.push(format!(
        "Catalog summary: {schema_count} schema(s), {relation_count} relation(s)."
    ));
    lines.push("Schema overview:".to_string());

    if nodes.is_empty() {
        lines.push("- <empty catalog>".to_string());
        return;
    }

    for node in nodes {
        match node.kind {
            ExplorerNodeKind::Schema => {
                let table_count = node
                    .children
                    .iter()
                    .filter(|child| matches!(child.kind, ExplorerNodeKind::Table))
                    .count();
                let view_count = node
                    .children
                    .iter()
                    .filter(|child| matches!(child.kind, ExplorerNodeKind::View))
                    .count();
                let relation_names = node
                    .children
                    .iter()
                    .map(|child| child.name.clone())
                    .collect::<Vec<_>>();
                let mut summary = format!(
                    "- schema {}: {} table(s), {} view(s)",
                    node.name, table_count, view_count
                );
                if !relation_names.is_empty() {
                    summary.push_str(&format!(" -> {}", relation_names.join(", ")));
                }
                lines.push(summary);
            }
            ExplorerNodeKind::Table | ExplorerNodeKind::View => {
                let kind = match node.kind {
                    ExplorerNodeKind::Table => "table",
                    ExplorerNodeKind::View => "view",
                    ExplorerNodeKind::Schema => unreachable!(),
                };
                lines.push(format!("- {kind}: {}", node.qualified_name));
            }
        }
    }
}

fn count_relations(node: &ExplorerNode) -> usize {
    match node.kind {
        ExplorerNodeKind::Schema => node.children.iter().map(count_relations).sum(),
        ExplorerNodeKind::Table | ExplorerNodeKind::View => 1,
    }
}

fn item_limit(item_count: usize, include_all_items: bool, default_limit: usize) -> usize {
    if include_all_items {
        item_count
    } else {
        default_limit
    }
}

fn append_limited_items(lines: &mut Vec<String>, items: &[String], limit: usize) {
    for item in items.iter().take(limit) {
        lines.push(format!("    - {item}"));
    }
    let overflow = items.len().saturating_sub(limit);
    if overflow > 0 {
        lines.push(format!("    - ... {overflow} more"));
    }
}

fn format_structure_item(name: &str, row_type: &str, target: &str, details: &str) -> String {
    let mut parts = Vec::new();
    if !row_type.trim().is_empty() {
        parts.push(inline_excerpt(row_type, MAX_INLINE_VALUE_LEN));
    }
    if !target.trim().is_empty() {
        parts.push(inline_excerpt(target, MAX_INLINE_VALUE_LEN));
    }
    if !details.trim().is_empty() {
        parts.push(inline_excerpt(details, MAX_INLINE_DETAILS_LEN));
    }

    if parts.is_empty() {
        name.to_string()
    } else if name.trim().is_empty() {
        parts.join(" · ")
    } else {
        format!("{name}: {}", parts.join(" · "))
    }
}

fn inline_excerpt(value: &str, max_len: usize) -> String {
    let single_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = single_line.trim();
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    let clipped = trimmed.chars().take(max_len).collect::<String>();
    format!("{clipped}...")
}

fn prioritize_table_sources(
    sources: Vec<TablePreviewSource>,
    focus_source: Option<TablePreviewSource>,
) -> Vec<TablePreviewSource> {
    let mut prioritized = Vec::new();
    if let Some(focus_source) = focus_source {
        prioritized.push(focus_source);
    }

    for source in sources {
        if prioritized
            .iter()
            .any(|existing| same_source(existing, &source))
        {
            continue;
        }
        prioritized.push(source);
    }

    prioritized
}

fn same_source(left: &TablePreviewSource, right: &TablePreviewSource) -> bool {
    left.qualified_name == right.qualified_name
}

fn collect_table_sources(nodes: &[ExplorerNode]) -> Vec<TablePreviewSource> {
    let mut sources = Vec::new();
    collect_table_sources_inner(nodes, &mut sources);
    sources
}

fn collect_table_sources_inner(nodes: &[ExplorerNode], sources: &mut Vec<TablePreviewSource>) {
    for node in nodes {
        match node.kind {
            ExplorerNodeKind::Table | ExplorerNodeKind::View => sources.push(TablePreviewSource {
                schema: node.schema.clone(),
                table_name: node.name.clone(),
                qualified_name: node.qualified_name.clone(),
            }),
            ExplorerNodeKind::Schema => collect_table_sources_inner(&node.children, sources),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_catalog_summary, append_observed_values, append_page_preview,
        append_structure_profile, build_catalog_signature, inline_excerpt,
        prioritize_table_sources, relation_heading,
    };
    use models::{ExplorerNode, ExplorerNodeKind, QueryPage, TablePreviewSource};

    #[test]
    fn page_preview_marks_partial_data_as_preview_only() {
        let page = QueryPage {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec!["1".to_string(), "Wireless Mouse".to_string()],
                vec!["2".to_string(), "Mechanical Keyboard".to_string()],
                vec!["3".to_string(), "USB-C Hub".to_string()],
            ],
            editable: None,
            offset: 0,
            page_size: 3,
            has_previous: false,
            has_next: true,
        };

        let mut lines = Vec::new();
        append_page_preview(&mut lines, &page);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("do not treat this as the full table")),
            "expected explicit preview-only wording, got: {lines:?}"
        );
    }

    #[test]
    fn page_preview_marks_totals_unknown_without_count() {
        let page = QueryPage {
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()]],
            editable: None,
            offset: 0,
            page_size: 10,
            has_previous: false,
            has_next: false,
        };

        let mut lines = Vec::new();
        append_page_preview(&mut lines, &page);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("totals are unknown unless counted explicitly")),
            "expected totals warning, got: {lines:?}"
        );
    }

    #[test]
    fn observed_values_collect_distinct_column_examples() {
        let page = QueryPage {
            columns: vec!["category".to_string(), "price".to_string()],
            rows: vec![
                vec!["Electronics".to_string(), "29.99".to_string()],
                vec!["Electronics".to_string(), "89.99".to_string()],
                vec!["Office".to_string(), "89.99".to_string()],
            ],
            editable: None,
            offset: 0,
            page_size: 3,
            has_previous: false,
            has_next: false,
        };

        let mut lines = Vec::new();
        append_observed_values(&mut lines, &page);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("category = Electronics | Office"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("price = 29.99 | 89.99"))
        );
    }

    #[test]
    fn focus_source_moves_to_front_without_duplicates() {
        let sources = vec![
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "orders".to_string(),
                qualified_name: "\"orders\"".to_string(),
            },
            TablePreviewSource {
                schema: Some("main".to_string()),
                table_name: "products".to_string(),
                qualified_name: "\"products\"".to_string(),
            },
        ];
        let focus = Some(TablePreviewSource {
            schema: Some("main".to_string()),
            table_name: "products".to_string(),
            qualified_name: "\"products\"".to_string(),
        });

        let prioritized = prioritize_table_sources(sources, focus);

        assert_eq!(prioritized[0].table_name, "products");
        assert_eq!(prioritized.len(), 2);
    }

    #[test]
    fn catalog_summary_lists_all_relation_names() {
        let nodes = vec![ExplorerNode {
            name: "main".to_string(),
            kind: ExplorerNodeKind::Schema,
            schema: Some("main".to_string()),
            qualified_name: "main".to_string(),
            children: (1..=10)
                .map(|index| ExplorerNode {
                    name: format!("table_{index}"),
                    kind: ExplorerNodeKind::Table,
                    schema: Some("main".to_string()),
                    qualified_name: format!("main.table_{index}"),
                    children: Vec::new(),
                })
                .collect(),
        }];

        let mut lines = Vec::new();
        append_catalog_summary(&mut lines, &nodes);
        let summary = lines.join("\n");

        assert!(summary.contains("table_1"));
        assert!(summary.contains("table_10"));
        assert!(!summary.contains("+"));
    }

    #[test]
    fn inline_excerpt_flattens_multiline_content() {
        let excerpt = inline_excerpt("CREATE TABLE products\n(\n  id INTEGER\n)", 80);
        assert_eq!(excerpt, "CREATE TABLE products ( id INTEGER )");
    }

    #[test]
    fn structure_profile_can_include_all_columns_for_full_schema_map() {
        let page = QueryPage {
            columns: vec![
                "section".to_string(),
                "name".to_string(),
                "type".to_string(),
                "target".to_string(),
                "details".to_string(),
            ],
            rows: (1..=10)
                .map(|index| {
                    vec![
                        "column".to_string(),
                        format!("col_{index}"),
                        "text".to_string(),
                        String::new(),
                        String::new(),
                    ]
                })
                .collect(),
            editable: None,
            offset: 0,
            page_size: 0,
            has_previous: false,
            has_next: false,
        };

        let mut lines = Vec::new();
        append_structure_profile(&mut lines, &page, true);
        let summary = lines.join("\n");

        assert!(summary.contains("col_1"));
        assert!(summary.contains("col_10"));
        assert!(!summary.contains("..."));
    }

    #[test]
    fn relation_heading_marks_active_focus() {
        let source = TablePreviewSource {
            schema: Some("main".to_string()),
            table_name: "products".to_string(),
            qualified_name: "\"main\".\"products\"".to_string(),
        };

        assert_eq!(
            relation_heading(&source, true),
            "- \"main\".\"products\" [active focus]"
        );
    }

    #[test]
    fn catalog_signature_changes_when_relation_names_change() {
        let nodes = vec![ExplorerNode {
            name: "main".to_string(),
            kind: ExplorerNodeKind::Schema,
            schema: Some("main".to_string()),
            qualified_name: "main".to_string(),
            children: vec![ExplorerNode {
                name: "products".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("main".to_string()),
                qualified_name: "main.products".to_string(),
                children: Vec::new(),
            }],
        }];
        let mut updated = nodes.clone();
        updated[0].children[0].qualified_name = "main.orders".to_string();
        updated[0].children[0].name = "orders".to_string();

        assert_ne!(
            build_catalog_signature(&nodes),
            build_catalog_signature(&updated)
        );
    }
}
