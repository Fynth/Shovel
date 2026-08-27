#![cfg_attr(not(test), allow(dead_code))]

use models::{ExplorerNode, ExplorerNodeKind};

use super::{
    keywords::{CompletionItem, CompletionKind},
    query::CompletionQuery,
};

pub fn schema_items(nodes: &[ExplorerNode], query: &CompletionQuery) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    if query.dotted.is_empty() {
        collect_schema_items(nodes, query, &mut items);
    } else {
        let table = &query.dotted[query.dotted.len() - 1];
        let schema =
            (query.dotted.len() >= 2).then(|| query.dotted[query.dotted.len() - 2].as_str());
        collect_dotted_columns(nodes, table, schema, query, &mut items);
    }
    items
}

pub fn merge_columns_into_tree(
    nodes: &mut [ExplorerNode],
    schema: Option<&str>,
    table: &str,
    columns: &[String],
) {
    merge_columns(nodes, schema, table, columns);
}

fn merge_columns(
    nodes: &mut [ExplorerNode],
    schema: Option<&str>,
    table: &str,
    columns: &[String],
) -> bool {
    for node in nodes {
        if is_relation(node.kind)
            && node.name.eq_ignore_ascii_case(table)
            && schema_matches(node.schema.as_deref(), schema)
        {
            if !node
                .children
                .iter()
                .any(|child| child.kind == ExplorerNodeKind::Column)
            {
                let qualified = node.qualified_name.clone();
                let table_schema = node.schema.clone();
                node.children = columns
                    .iter()
                    .map(|col| ExplorerNode {
                        name: col.clone(),
                        kind: ExplorerNodeKind::Column,
                        schema: table_schema.clone(),
                        qualified_name: format!("{qualified}.{col}"),
                        row_count: None,
                        children: Vec::new(),
                    })
                    .collect();
            }
            return true;
        }
        if merge_columns(&mut node.children, schema, table, columns) {
            return true;
        }
    }
    false
}

fn collect_schema_items(
    nodes: &[ExplorerNode],
    query: &CompletionQuery,
    items: &mut Vec<CompletionItem>,
) {
    for node in nodes {
        if let Some(kind) = completion_kind(node.kind) {
            items.push(item_from_node(node, kind, query));
        }
        collect_schema_items(&node.children, query, items);
    }
}

fn collect_dotted_columns(
    nodes: &[ExplorerNode],
    table: &str,
    schema: Option<&str>,
    query: &CompletionQuery,
    items: &mut Vec<CompletionItem>,
) {
    for node in nodes {
        if is_relation(node.kind)
            && node.name.eq_ignore_ascii_case(table)
            && schema_matches(node.schema.as_deref(), schema)
        {
            for child in &node.children {
                if child.kind == ExplorerNodeKind::Column {
                    items.push(item_from_node(child, CompletionKind::Column, query));
                }
            }
        }
        collect_dotted_columns(&node.children, table, schema, query, items);
    }
}

fn item_from_node(
    node: &ExplorerNode,
    kind: CompletionKind,
    query: &CompletionQuery,
) -> CompletionItem {
    CompletionItem {
        label: node.name.clone(),
        detail: node.qualified_name.clone(),
        kind,
        replace: query.token_range.start..query.token_range.end,
    }
}

fn completion_kind(kind: ExplorerNodeKind) -> Option<CompletionKind> {
    match kind {
        ExplorerNodeKind::Schema => Some(CompletionKind::Schema),
        ExplorerNodeKind::Table => Some(CompletionKind::Table),
        ExplorerNodeKind::View => Some(CompletionKind::View),
        ExplorerNodeKind::Function => Some(CompletionKind::Function),
        ExplorerNodeKind::Procedure => Some(CompletionKind::Procedure),
        ExplorerNodeKind::Column => Some(CompletionKind::Column),
        ExplorerNodeKind::MaterializedView
        | ExplorerNodeKind::Sequence
        | ExplorerNodeKind::Trigger => None,
    }
}

fn is_relation(kind: ExplorerNodeKind) -> bool {
    matches!(
        kind,
        ExplorerNodeKind::Table | ExplorerNodeKind::View | ExplorerNodeKind::MaterializedView
    )
}

fn schema_matches(got: Option<&str>, expected: Option<&str>) -> bool {
    match expected {
        None => true,
        Some(expected) => got.is_some_and(|got| got.eq_ignore_ascii_case(expected)),
    }
}
