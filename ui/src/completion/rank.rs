#![cfg_attr(not(test), allow(dead_code))]

use models::{DatabaseKind, ExplorerNode};

use super::{
    keywords::{CompletionItem, CompletionKind, keyword_items},
    query::{CompletionClause, CompletionQuery},
    schema::schema_items,
};

pub fn filter_and_rank(items: Vec<CompletionItem>, query: &CompletionQuery) -> Vec<CompletionItem> {
    let token = query.token.to_ascii_lowercase();
    let mut scored: Vec<(i32, CompletionItem)> = items
        .into_iter()
        .filter(|item| token.is_empty() || item.label.to_ascii_lowercase().contains(&token))
        .map(|item| {
            let label_lower = item.label.to_ascii_lowercase();
            let mut score = if token.is_empty() || label_lower.starts_with(&token) {
                200
            } else {
                100
            };
            if is_preferred(item.kind, query) {
                score += 50;
            }
            (score, item)
        })
        .collect();

    scored.sort_by(|(score_a, a), (score_b, b)| {
        score_b.cmp(score_a).then_with(|| {
            a.label.len().cmp(&b.label.len()).then_with(|| {
                a.label
                    .to_ascii_lowercase()
                    .cmp(&b.label.to_ascii_lowercase())
            })
        })
    });

    scored.into_iter().map(|(_, item)| item).take(50).collect()
}

pub fn collect_menu_items(
    kind: DatabaseKind,
    nodes: &[ExplorerNode],
    query: &CompletionQuery,
    force: bool,
) -> Vec<CompletionItem> {
    if !force && query.token.is_empty() && query.dotted.is_empty() {
        return Vec::new();
    }

    // After `table.`, only relation columns belong in the menu.
    let mut items = if query.dotted.is_empty() {
        keyword_items(kind, query)
    } else {
        Vec::new()
    };
    items.extend(schema_items(nodes, query));
    filter_and_rank(items, query)
}

pub fn apply_menu_item(sql: &str, item: &CompletionItem) -> (String, usize) {
    let next = format!(
        "{}{}{}",
        &sql[..item.replace.start],
        item.label,
        &sql[item.replace.end..]
    );
    let cursor = item.replace.start + item.label.len();
    (next, cursor)
}

fn is_preferred(kind: CompletionKind, query: &CompletionQuery) -> bool {
    if !query.dotted.is_empty() {
        return kind == CompletionKind::Column;
    }
    match query.clause {
        CompletionClause::From => matches!(
            kind,
            CompletionKind::Schema | CompletionKind::Table | CompletionKind::View
        ),
        CompletionClause::Column => matches!(kind, CompletionKind::Column | CompletionKind::Table),
        CompletionClause::Call =>
            matches!(kind, CompletionKind::Function | CompletionKind::Procedure),
        CompletionClause::Other => matches!(
            kind,
            CompletionKind::Keyword | CompletionKind::Table | CompletionKind::View
        ),
    }
}

#[cfg(test)]
mod tests {
    use models::{DatabaseKind, ExplorerNode, ExplorerNodeKind};

    use super::{
        super::{
            keywords::{CompletionItem, CompletionKind},
            query::parse_completion_query,
            schema::merge_columns_into_tree,
        },
        apply_menu_item,
        collect_menu_items,
        filter_and_rank,
    };

    fn table(name: &str, columns: &[&str]) -> ExplorerNode {
        ExplorerNode {
            name: name.into(),
            kind: ExplorerNodeKind::Table,
            schema: None,
            qualified_name: name.into(),
            row_count: None,
            children: columns
                .iter()
                .map(|col| ExplorerNode {
                    name: (*col).into(),
                    kind: ExplorerNodeKind::Column,
                    schema: None,
                    qualified_name: format!("{name}.{col}"),
                    row_count: None,
                    children: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn from_clause_ranks_tables_above_columns() {
        let nodes = vec![table("users", &["name"]), table("orders", &[])];
        let query = parse_completion_query("SELECT * FROM u", 15);
        let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
        assert_eq!(items[0].label, "users");
        assert_eq!(items[0].kind, CompletionKind::Table);
    }

    #[test]
    fn dotted_prefix_prefers_columns() {
        let nodes = vec![table("users", &["id", "name"])];
        let sql = "SELECT * FROM users.";
        let query = parse_completion_query(sql, sql.len());
        let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
        assert!(items.iter().all(|item| item.kind == CompletionKind::Column));
        assert!(items.iter().any(|item| item.label == "name"));
    }

    #[test]
    fn filter_prefix_beats_substring_and_caps_at_50() {
        let mut items: Vec<CompletionItem> = (0..80)
            .map(|i| CompletionItem {
                label: format!("col{i:02}"),
                detail: String::new(),
                kind: CompletionKind::Column,
                replace: 0..0,
            })
            .collect();
        items.push(CompletionItem {
            label: "id".into(),
            detail: String::new(),
            kind: CompletionKind::Column,
            replace: 0..0,
        });
        let query = parse_completion_query("SELECT i", 8);
        let ranked = filter_and_rank(items, &query);
        assert_eq!(ranked[0].label, "id");
        assert!(ranked.len() <= 50);
    }

    #[test]
    fn empty_token_without_force_or_dot_is_empty() {
        let nodes = vec![table("users", &[])];
        let query = parse_completion_query("SELECT ", 7);
        let items = collect_menu_items(DatabaseKind::Sqlite, &nodes, &query, false);
        assert!(items.is_empty());
    }

    #[test]
    fn apply_menu_item_replaces_token() {
        let sql = "SELECT * FROM us";
        let query = parse_completion_query(sql, sql.len());
        let item = CompletionItem {
            label: "users".into(),
            detail: String::new(),
            kind: CompletionKind::Table,
            replace: query.token_range.clone(),
        };
        let (next, cursor) = apply_menu_item(sql, &item);
        assert_eq!(next, "SELECT * FROM users");
        assert_eq!(cursor, next.len());
    }

    #[test]
    fn merge_columns_into_tree_appends_column_children() {
        let mut nodes = vec![table("users", &[])];
        merge_columns_into_tree(&mut nodes, None, "users", &["id".into(), "name".into()]);
        assert_eq!(nodes[0].children.len(), 2);
        assert_eq!(nodes[0].children[0].kind, ExplorerNodeKind::Column);
    }
}
