// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::ExplainExec;
use models::{DatabaseError, ExecutionPlan, ExecutionPlanNode};
use sqlx::Row;

use crate::session::SqliteSession;

#[async_trait]
impl ExplainExec for SqliteSession {
    async fn execute_explain(
        &self,
        sql: &str,
        _analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        execute_sqlite_explain(&self.pool, trimmed).await
    }
}

async fn execute_sqlite_explain(
    pool: &sqlx::SqlitePool,
    sql: &str,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = sqlite_explain_query_plan_sql(sql);
    let rows = sqlx::query(&explain_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut raw_lines: Vec<String> = Vec::new();
    let mut entries: Vec<(i64, i64, String)> = Vec::new();

    for row in &rows {
        let id: i64 = row.try_get("id").unwrap_or(0);
        let parent: i64 = row.try_get("parent").unwrap_or(0);
        let detail: String = row.try_get("detail").unwrap_or_default();
        raw_lines.push(format!("id={id} parent={parent} | {detail}"));
        entries.push((id, parent, detail));
    }

    let root_nodes = build_sqlite_plan_tree(&entries);
    let mut plan = ExecutionPlan::new(sql);
    plan.root_nodes = root_nodes;
    plan.raw_text = raw_lines;
    Ok(plan)
}

fn sqlite_explain_query_plan_sql(sql: &str) -> String {
    let trimmed = sql.trim();
    let Some(after_explain) = strip_leading_sql_keyword(trimmed, "explain") else {
        return format!("EXPLAIN QUERY PLAN {trimmed}");
    };

    if let Some(after_query) = strip_leading_sql_keyword(after_explain, "query")
        && strip_leading_sql_keyword(after_query, "plan").is_some()
    {
        return trimmed.to_string();
    }

    format!("EXPLAIN QUERY PLAN {}", after_explain.trim())
}

fn strip_leading_sql_keyword<'a>(sql: &'a str, keyword: &str) -> Option<&'a str> {
    let trimmed = sql.trim_start();
    let prefix = trimmed.get(..keyword.len())?;
    if !prefix.eq_ignore_ascii_case(keyword) {
        return None;
    }

    let rest = &trimmed[keyword.len()..];
    if rest
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }

    Some(rest.trim_start())
}

/// Build a tree from SQLite EXPLAIN QUERY PLAN rows.
///
/// SQLite returns flat rows with (id, parent, detail). The tree is built by
/// looking up each row's parent. Rows with parent == 0 are roots. If a parent
/// id doesn't exist as a child row we attach it to the nearest ancestor.
fn build_sqlite_plan_tree(entries: &[(i64, i64, String)]) -> Vec<ExecutionPlanNode> {
    if entries.is_empty() {
        return Vec::new();
    }

    // Collect child IDs per parent.
    let mut children_of: std::collections::HashMap<i64, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, &(_id, parent, _)) in entries.iter().enumerate() {
        children_of.entry(parent).or_default().push(idx);
    }

    // Build nodes recursively starting from parent == 0.
    fn build_node(
        entries: &[(i64, i64, String)],
        idx: usize,
        children_of: &std::collections::HashMap<i64, Vec<usize>>,
    ) -> ExecutionPlanNode {
        let (id, _parent, detail) = &entries[idx];
        let node = parse_sqlite_detail(detail);

        let child_indices = children_of.get(id).cloned().unwrap_or_default();
        let children: Vec<ExecutionPlanNode> = child_indices
            .iter()
            .map(|&ci| build_node(entries, ci, children_of))
            .collect();

        let mut node = node;
        node.children = children;
        node
    }

    // Find root entries (those with parent == 0, excluding the synthetic root if present).
    let root_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|&(_, &(id, parent, _))| parent == 0 && id != 0)
        .map(|(idx, _)| idx)
        .collect();

    // If all entries have non-zero parents or there's a single id=0 root, handle that.
    if root_indices.is_empty() {
        // Everything might be under a single root (id=0, parent=0).
        if let Some(_root_idx) = entries
            .iter()
            .position(|&(id, parent, _)| id == 0 && parent == 0)
        {
            let child_indices = children_of.get(&0).cloned().unwrap_or_default();
            return child_indices
                .iter()
                .map(|&ci| build_node(entries, ci, &children_of))
                .collect();
        }
        // Fallback: treat all as roots.
        return entries
            .iter()
            .enumerate()
            .map(|(idx, _)| build_node(entries, idx, &children_of))
            .collect();
    }

    root_indices
        .iter()
        .map(|&idx| build_node(entries, idx, &children_of))
        .collect()
}

/// Parse a single SQLite EXPLAIN QUERY PLAN detail line.
///
/// Examples:
///   "SCAN users"
///   "SEARCH users USING INDEX idx_name (id=?)"
///   "USE TEMP B-TREE FOR ORDER BY"
///   "EXECUTE LIST SUBQUERY 1"
///   "COMPOUND SUBQUERIES 1 AND 2 USING TEMP TABLE (UNION)"
fn parse_sqlite_detail(detail: &str) -> ExecutionPlanNode {
    let detail = detail.trim();
    if detail.is_empty() {
        return ExecutionPlanNode::new("unknown").with_raw_text(detail);
    }

    let upper = detail.to_ascii_uppercase();

    if upper.starts_with("SCAN ") {
        let source = detail["SCAN ".len()..].trim();
        if is_sqlite_values_clause_scan(&upper) {
            let mut node = ExecutionPlanNode::new("Constant Rows")
                .with_detail("source", "VALUES clause")
                .with_raw_text(detail);
            if let Some(rows) = parse_sqlite_constant_row_count(source) {
                node = node.with_rows(rows);
            }
            return node;
        }

        let table = detail["SCAN ".len()..].trim();
        let table_name = extract_first_identifier(table);
        return ExecutionPlanNode::new("Scan")
            .with_target(table_name.unwrap_or(table))
            .with_detail("type", "full table scan")
            .with_raw_text(detail);
    }

    if let Some(upper_rest) = upper.strip_prefix("SEARCH ") {
        let rest_original = &detail["SEARCH ".len()..];
        let table_name = extract_first_identifier(rest_original);
        let mut node = ExecutionPlanNode::new("Search").with_raw_text(detail);

        if let Some(table) = table_name {
            node = node.with_target(table);
        }

        // Check for index usage.
        if let Some(idx_pos) = upper_rest.find("USING COVERING INDEX") {
            let index_info = rest_original[idx_pos..].trim();
            node = node.with_detail("covering index", index_info);
        } else if let Some(idx_pos) = upper_rest.find("USING INDEX") {
            let index_info = rest_original[idx_pos..].trim();
            node = node.with_detail("index", index_info);
        }

        return node;
    }

    if upper.starts_with("USE TEMP B-TREE") {
        return ExecutionPlanNode::new("Temp B-Tree")
            .with_detail("purpose", detail)
            .with_raw_text(detail);
    }

    if upper.starts_with("EXECUTE ") {
        return ExecutionPlanNode::new("Subquery")
            .with_detail("type", detail)
            .with_raw_text(detail);
    }

    if upper.starts_with("COMPOUND SUBQUERIES") {
        return ExecutionPlanNode::new("Compound Subqueries")
            .with_detail("type", detail)
            .with_raw_text(detail);
    }

    // Generic fallback.
    ExecutionPlanNode::new("Operation").with_raw_text(detail)
}

/// Extract the first whitespace-delimited identifier from a string.
fn extract_first_identifier(s: &str) -> Option<&str> {
    s.split_whitespace().next().filter(|word| !word.is_empty())
}

fn parse_sqlite_constant_row_count(source: &str) -> Option<u64> {
    let token = source.split_whitespace().next()?;
    let digits = token
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u64>().ok()
}

fn is_sqlite_values_clause_scan(upper: &str) -> bool {
    upper.ends_with(" VALUES CLAUSE")
        || upper.ends_with(" CONSTANT")
        || upper.ends_with(" CONSTANT ROW")
        || upper.ends_with(" CONSTANT ROWS")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SqliteDriver, SqliteSession};
    use database::{DatabaseDriver, SessionHandle};
    use std::sync::Arc;

    #[test]
    fn sqlite_plan_tree_builds_simple_scan() {
        let entries = vec![
            (0, 0, String::new()), // synthetic root
            (1, 0, "SCAN users".to_string()),
            (
                2,
                0,
                "SEARCH posts USING INDEX idx_posts_user_id (user_id=?)".to_string(),
            ),
        ];
        let roots = build_sqlite_plan_tree(&entries);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].operation, "Scan");
        assert_eq!(roots[0].target.as_deref(), Some("users"));
        assert_eq!(roots[1].operation, "Search");
        assert_eq!(roots[1].target.as_deref(), Some("posts"));
    }

    #[test]
    fn sqlite_plan_tree_builds_nested() {
        let entries = vec![
            (0, 0, String::new()),
            (1, 0, "SCAN users".to_string()),
            (3, 1, "USE TEMP B-TREE FOR ORDER BY".to_string()),
        ];
        let roots = build_sqlite_plan_tree(&entries);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].operation, "Scan");
        assert_eq!(roots[0].children.len(), 1);
        assert_eq!(roots[0].children[0].operation, "Temp B-Tree");
    }

    #[test]
    fn sqlite_detail_parsing() {
        let node = parse_sqlite_detail("SCAN users");
        assert_eq!(node.operation, "Scan");

        let node = parse_sqlite_detail("SCAN 3-ROW VALUES CLAUSE");
        assert_eq!(node.operation, "Constant Rows");
        assert_eq!(node.estimated_rows, Some(3));
        assert!(
            node.details
                .iter()
                .any(|(k, v)| k == "source" && v == "VALUES clause")
        );

        let node = parse_sqlite_detail("SEARCH posts USING INDEX idx_user (user_id=?)");
        assert_eq!(node.operation, "Search");
        assert!(node.details.iter().any(|(k, _)| k == "index"));

        let node = parse_sqlite_detail("USE TEMP B-TREE FOR ORDER BY");
        assert_eq!(node.operation, "Temp B-Tree");

        let node = parse_sqlite_detail("EXECUTE LIST SUBQUERY 1");
        assert_eq!(node.operation, "Subquery");

        let node = parse_sqlite_detail("COMPOUND SUBQUERIES 1 AND 2 USING TEMP TABLE (UNION)");
        assert_eq!(node.operation, "Compound Subqueries");
    }

    #[test]
    fn sqlite_explain_sql_does_not_double_prefix_existing_explain() {
        assert_eq!(
            sqlite_explain_query_plan_sql("select * from users"),
            "EXPLAIN QUERY PLAN select * from users"
        );
        assert_eq!(
            sqlite_explain_query_plan_sql("EXPLAIN QUERY PLAN select * from users"),
            "EXPLAIN QUERY PLAN select * from users"
        );
        assert_eq!(
            sqlite_explain_query_plan_sql("EXPLAIN select * from users"),
            "EXPLAIN QUERY PLAN select * from users"
        );
    }

    #[tokio::test]
    async fn sqlite_execute_explain_accepts_existing_explain_statement() {
        let pool = SqliteDriver::connect(":memory:".into()).await.unwrap();
        sqlx::query("create table users (id integer primary key, name text)")
            .execute(&pool)
            .await
            .unwrap();

        let handle = SessionHandle::wrap(Arc::new(SqliteSession { pool }));
        let plan = handle
            .explain()
            .expect("sqlite explain")
            .execute_explain("EXPLAIN select * from users", false)
            .await
            .unwrap();

        assert_eq!(plan.root_nodes.len(), 1);
        assert_eq!(plan.root_nodes[0].operation, "Scan");
        assert_eq!(plan.root_nodes[0].target.as_deref(), Some("users"));
    }
}
