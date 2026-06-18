use models::{DatabaseConnection, ExplorerNode, TablePreviewSource};
use services::{load_connection_tree, preview_source_for_sql};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct ResolvedAgentSql {
    pub(super) sql: String,
    pub(super) correction_note: Option<String>,
}

pub(super) async fn resolve_agent_sql_execution(
    connection: DatabaseConnection,
    sql: &str,
) -> Result<ResolvedAgentSql, String> {
    let DatabaseConnection::ClickHouse(config) = &connection else {
        return Ok(ResolvedAgentSql {
            sql: sql.to_string(),
            correction_note: None,
        });
    };
    let Some(source) = preview_source_for_sql(sql) else {
        return Ok(ResolvedAgentSql {
            sql: sql.to_string(),
            correction_note: None,
        });
    };
    let default_schema = config.effective_database().to_string();
    let tree = load_connection_tree(connection).await.map_err(|err| {
        format!("Failed to refresh ClickHouse catalog before running ACP SQL: {err}")
    })?;

    if clickhouse_catalog_contains_source(&tree, &source, &default_schema) {
        return Ok(ResolvedAgentSql {
            sql: sql.to_string(),
            correction_note: None,
        });
    }

    let relation = clickhouse_source_display_name(&source, &default_schema);
    let matches = ranked_clickhouse_source_matches(&tree, &source, &default_schema);
    if let Some(best_match) = matches.first()
        && clickhouse_match_is_confident(best_match, matches.get(1))
    {
        let corrected_sql = rewrite_simple_select_source(sql, &source, &best_match.source);
        let corrected_relation =
            clickhouse_source_display_name(&best_match.source, &default_schema);
        return Ok(ResolvedAgentSql {
            sql: corrected_sql,
            correction_note: Some(format!(
                "Corrected relation `{relation}` to `{corrected_relation}` using the current ClickHouse catalog."
            )),
        });
    }

    let suggestions = matches
        .iter()
        .take(3)
        .map(|candidate| clickhouse_source_display_name(&candidate.source, &default_schema))
        .collect::<Vec<_>>();
    let suggestion_suffix = if suggestions.is_empty() {
        String::new()
    } else {
        format!(" Closest matches: {}.", suggestions.join(", "))
    };
    Err(format!(
        "ACP generated SQL for `{relation}`, but that relation is not available in the current ClickHouse catalog. Use an exact table or view name from the database snapshot.{suggestion_suffix}"
    ))
}

#[derive(Clone, Debug)]
struct ClickHouseSourceMatch {
    source: TablePreviewSource,
    score: usize,
    shared_token_weight: usize,
}

fn clickhouse_catalog_contains_source(
    nodes: &[ExplorerNode],
    source: &TablePreviewSource,
    default_schema: &str,
) -> bool {
    let expected_schema = source.schema.as_deref().unwrap_or(default_schema);
    nodes.iter().any(|node| match node.kind {
        models::ExplorerNodeKind::Schema =>
            node.name == expected_schema
                && node.children.iter().any(|child| {
                    child.schema.as_deref() == Some(expected_schema)
                        && child.name == source.table_name
                }),
        _ => node.schema.as_deref() == Some(expected_schema) && node.name == source.table_name,
    })
}

fn clickhouse_source_display_name(source: &TablePreviewSource, default_schema: &str) -> String {
    source
        .schema
        .as_deref()
        .map(|schema| format!("{schema}.{}", source.table_name))
        .unwrap_or_else(|| format!("{default_schema}.{}", source.table_name))
}

fn ranked_clickhouse_source_matches(
    nodes: &[ExplorerNode],
    source: &TablePreviewSource,
    default_schema: &str,
) -> Vec<ClickHouseSourceMatch> {
    let expected_schema = source.schema.as_deref().unwrap_or(default_schema);
    let mut matches = collect_clickhouse_catalog_sources(nodes)
        .into_iter()
        .filter(|candidate| candidate.table_name != source.table_name)
        .map(|candidate| {
            let shared_token_weight =
                shared_identifier_token_weight(&source.table_name, &candidate.table_name);
            let bigram_overlap = shared_bigram_count(&source.table_name, &candidate.table_name);
            let schema_bonus =
                usize::from(candidate.schema.as_deref() == Some(expected_schema)) * 10_000;
            let score = schema_bonus + shared_token_weight * 100 + bigram_overlap;
            ClickHouseSourceMatch {
                source: candidate,
                score,
                shared_token_weight,
            }
        })
        .filter(|candidate| candidate.shared_token_weight > 0)
        .collect::<Vec<_>>();

    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.shared_token_weight.cmp(&left.shared_token_weight))
            .then_with(|| {
                left.source
                    .table_name
                    .len()
                    .cmp(&right.source.table_name.len())
            })
            .then_with(|| left.source.table_name.cmp(&right.source.table_name))
    });
    matches
}

fn clickhouse_match_is_confident(
    best_match: &ClickHouseSourceMatch,
    runner_up: Option<&ClickHouseSourceMatch>,
) -> bool {
    if best_match.shared_token_weight < 40 {
        return false;
    }

    runner_up.is_none_or(|next| best_match.score >= next.score + 150)
}

fn collect_clickhouse_catalog_sources(nodes: &[ExplorerNode]) -> Vec<TablePreviewSource> {
    let mut sources = Vec::new();
    collect_clickhouse_catalog_sources_inner(nodes, &mut sources);
    sources
}

fn collect_clickhouse_catalog_sources_inner(
    nodes: &[ExplorerNode],
    sources: &mut Vec<TablePreviewSource>,
) {
    for node in nodes {
        match node.kind {
            models::ExplorerNodeKind::Schema => {
                collect_clickhouse_catalog_sources_inner(&node.children, sources);
            }
            _ => sources.push(TablePreviewSource {
                schema: node.schema.clone(),
                table_name: node.name.clone(),
                qualified_name: node.qualified_name.clone(),
            }),
        }
    }
}

fn shared_identifier_token_weight(left: &str, right: &str) -> usize {
    let left_tokens = identifier_token_set(left);
    let right_tokens = identifier_token_set(right);
    left_tokens
        .intersection(&right_tokens)
        .map(|token| token.len() * token.len())
        .sum()
}

fn identifier_token_set(value: &str) -> BTreeSet<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            (token.len() >= 3).then_some(token)
        })
        .collect()
}

fn shared_bigram_count(left: &str, right: &str) -> usize {
    let left_bigrams = identifier_bigrams(left);
    let right_bigrams = identifier_bigrams(right);
    left_bigrams.intersection(&right_bigrams).count()
}

fn identifier_bigrams(value: &str) -> BTreeSet<String> {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<Vec<_>>();
    normalized
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn rewrite_simple_select_source(
    sql: &str,
    source: &TablePreviewSource,
    replacement: &TablePreviewSource,
) -> String {
    sql.replacen(&source.qualified_name, &replacement.qualified_name, 1)
}

#[cfg(test)]
mod tests {
    use super::{
        clickhouse_catalog_contains_source,
        clickhouse_match_is_confident,
        clickhouse_source_display_name,
        ranked_clickhouse_source_matches,
        rewrite_simple_select_source,
    };
    use models::{ExplorerNode, ExplorerNodeKind, TablePreviewSource};

    #[test]
    fn clickhouse_catalog_lookup_uses_default_schema_for_unqualified_sql() {
        let tree = vec![ExplorerNode {
            name: "dwh_ogs".to_string(),
            kind: ExplorerNodeKind::Schema,
            schema: Some("dwh_ogs".to_string()),
            qualified_name: "`dwh_ogs`".to_string(),
            children: vec![ExplorerNode {
                name: "source_statistics".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("dwh_ogs".to_string()),
                qualified_name: "`dwh_ogs`.`source_statistics`".to_string(),
                children: Vec::new(),
            }],
        }];
        let source = TablePreviewSource {
            schema: None,
            table_name: "source_statistics".to_string(),
            qualified_name: "source_statistics".to_string(),
        };

        assert!(clickhouse_catalog_contains_source(
            &tree, &source, "dwh_ogs"
        ));
        assert_eq!(
            clickhouse_source_display_name(&source, "dwh_ogs"),
            "dwh_ogs.source_statistics"
        );
    }

    #[test]
    fn clickhouse_catalog_lookup_rejects_missing_relation_names() {
        let tree = vec![ExplorerNode {
            name: "dwh_ogs".to_string(),
            kind: ExplorerNodeKind::Schema,
            schema: Some("dwh_ogs".to_string()),
            qualified_name: "`dwh_ogs`".to_string(),
            children: vec![ExplorerNode {
                name: "dag_source_statistics".to_string(),
                kind: ExplorerNodeKind::Table,
                schema: Some("dwh_ogs".to_string()),
                qualified_name: "`dwh_ogs`.`dag_source_statistics`".to_string(),
                children: Vec::new(),
            }],
        }];
        let source = TablePreviewSource {
            schema: Some("dwh_ogs".to_string()),
            table_name: "dag_source_statistics_kafka_buffer".to_string(),
            qualified_name: "dwh_ogs.dag_source_statistics_kafka_buffer".to_string(),
        };

        assert!(!clickhouse_catalog_contains_source(
            &tree, &source, "dwh_ogs"
        ));
    }

    #[test]
    fn clickhouse_matcher_prefers_real_buffer_table_with_strongest_token_overlap() {
        let tree = vec![ExplorerNode {
            name: "dwh_ogs".to_string(),
            kind: ExplorerNodeKind::Schema,
            schema: Some("dwh_ogs".to_string()),
            qualified_name: "`dwh_ogs`".to_string(),
            children: vec![
                ExplorerNode {
                    name: "dag_source_statistics".to_string(),
                    kind: ExplorerNodeKind::Table,
                    schema: Some("dwh_ogs".to_string()),
                    qualified_name: "`dwh_ogs`.`dag_source_statistics`".to_string(),
                    children: Vec::new(),
                },
                ExplorerNode {
                    name: "source_statistics_test_debug_buffer".to_string(),
                    kind: ExplorerNodeKind::Table,
                    schema: Some("dwh_ogs".to_string()),
                    qualified_name: "`dwh_ogs`.`source_statistics_test_debug_buffer`".to_string(),
                    children: Vec::new(),
                },
            ],
        }];
        let source = TablePreviewSource {
            schema: Some("dwh_ogs".to_string()),
            table_name: "dag_source_statistics_kafka_buffer".to_string(),
            qualified_name: "dwh_ogs.dag_source_statistics_kafka_buffer".to_string(),
        };

        let matches = ranked_clickhouse_source_matches(&tree, &source, "dwh_ogs");
        assert_eq!(
            matches.first().unwrap().source.table_name,
            "source_statistics_test_debug_buffer"
        );
        assert!(clickhouse_match_is_confident(
            matches.first().unwrap(),
            matches.get(1)
        ));
    }

    #[test]
    fn rewrite_simple_select_source_swaps_relation_name_once() {
        let sql = "SELECT * FROM dwh_ogs.dag_source_statistics_kafka_buffer";
        let source = TablePreviewSource {
            schema: Some("dwh_ogs".to_string()),
            table_name: "dag_source_statistics_kafka_buffer".to_string(),
            qualified_name: "dwh_ogs.dag_source_statistics_kafka_buffer".to_string(),
        };
        let replacement = TablePreviewSource {
            schema: Some("dwh_ogs".to_string()),
            table_name: "source_statistics_test_debug_buffer".to_string(),
            qualified_name: "`dwh_ogs`.`source_statistics_test_debug_buffer`".to_string(),
        };

        assert_eq!(
            rewrite_simple_select_source(sql, &source, &replacement),
            "SELECT * FROM `dwh_ogs`.`source_statistics_test_debug_buffer`"
        );
    }
}
