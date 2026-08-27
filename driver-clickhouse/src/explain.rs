// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::ExplainExec;
use models::{DatabaseError, ExecutionPlan, ExecutionPlanNode};

use crate::session::ClickHouseSession;

#[async_trait]
impl ExplainExec for ClickHouseSession {
    async fn execute_explain(
        &self,
        sql: &str,
        _analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        let explain_sql = format!("EXPLAIN {trimmed}");
        let raw_text = crate::execute_text_query(&self.config, &explain_sql)
            .await
            .map_err(DatabaseError::Driver)?;

        let raw_lines: Vec<String> = raw_text.lines().map(String::from).collect();
        let root_nodes = parse_clickhouse_plan_text(&raw_text);

        let mut plan = ExecutionPlan::new(trimmed);
        plan.root_nodes = root_nodes;
        plan.raw_text = raw_lines;
        Ok(plan)
    }
}

fn parse_clickhouse_plan_text(text: &str) -> Vec<ExecutionPlanNode> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let indent_unit = detect_indent_unit(&lines);
    let items: Vec<(usize, ExecutionPlanNode)> = lines
        .iter()
        .map(|line| {
            let (depth, content) = measure_depth(line, indent_unit);
            let node = parse_clickhouse_line(content);
            (depth, node)
        })
        .filter(|(_, node)| {
            node.operation != "unknown" || !node.raw_text.as_ref().is_none_or(|t| t.is_empty())
        })
        .collect();

    build_tree_from_stack(&items)
}

fn detect_indent_unit(lines: &[&str]) -> usize {
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed == *line {
            continue;
        }
        let leading = line.len() - trimmed.len();
        if leading > 0 {
            return leading;
        }
    }
    2
}

fn measure_depth(line: &str, indent_unit: usize) -> (usize, &str) {
    let content = line.trim_start();
    if content.is_empty() {
        return (0, "");
    }
    let leading = line.len() - content.len();
    if indent_unit == 0 {
        return (0, content);
    }
    let depth = leading / indent_unit;
    (depth, content)
}

fn parse_clickhouse_line(line: &str) -> ExecutionPlanNode {
    let line = line.trim();
    if line.is_empty() {
        return ExecutionPlanNode::new("unknown");
    }

    if let Some(paren_start) = line.find('(')
        && line.ends_with(')')
    {
        let operation = line[..paren_start].trim();
        let detail = &line[(paren_start + 1)..(line.len() - 1)];
        let mut node = ExecutionPlanNode::new(operation);

        if operation.eq_ignore_ascii_case("ReadFromMergeTree")
            || operation.eq_ignore_ascii_case("ReadFromStorage")
        {
            node = node.with_target(detail);
        }

        return node.with_detail("detail", detail);
    }

    ExecutionPlanNode::new(line)
}

fn build_tree_from_stack(items: &[(usize, ExecutionPlanNode)]) -> Vec<ExecutionPlanNode> {
    if items.is_empty() {
        return Vec::new();
    }

    let n = items.len();
    let mut children_indices: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut parent_of: Vec<Option<usize>> = vec![None; n];

    for i in 0..n {
        let (depth_i, _) = &items[i];
        for j in (0..i).rev() {
            let (depth_j, _) = &items[j];
            if *depth_j < *depth_i {
                parent_of[i] = Some(j);
                children_indices[j].push(i);
                break;
            }
        }
    }

    let roots: Vec<usize> = (0..n).filter(|&i| parent_of[i].is_none()).collect();

    fn build(
        idx: usize,
        items: &[(usize, ExecutionPlanNode)],
        children_indices: &[Vec<usize>],
    ) -> ExecutionPlanNode {
        let mut node = items[idx].1.clone();
        node.children = children_indices[idx]
            .iter()
            .map(|&ci| build(ci, items, children_indices))
            .collect();
        node
    }

    roots
        .into_iter()
        .map(|r| build(r, items, &children_indices))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clickhouse_text_parsing() {
        let text = "\
Expression
  Filter
    ReadFromMergeTree (default.users)";

        let roots = parse_clickhouse_plan_text(text);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].operation, "Expression");
        assert_eq!(roots[0].children.len(), 1);
        assert_eq!(roots[0].children[0].operation, "Filter");
        assert_eq!(roots[0].children[0].children.len(), 1);
        assert_eq!(
            roots[0].children[0].children[0].operation,
            "ReadFromMergeTree"
        );
        assert_eq!(
            roots[0].children[0].children[0].target.as_deref(),
            Some("default.users")
        );
    }

    #[test]
    fn clickhouse_text_parsing_multiple_roots() {
        let text = "\
Expression (Projection)
Expression (Before ORDER BY)
  Sorting (Sorting by expression)
    ReadFromMergeTree (default.table)";

        let roots = parse_clickhouse_plan_text(text);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].operation, "Expression");
        assert_eq!(roots[1].operation, "Expression");
        assert_eq!(roots[1].children.len(), 1);
        assert_eq!(roots[1].children[0].operation, "Sorting");
    }

    #[test]
    fn clickhouse_empty_text() {
        let roots = parse_clickhouse_plan_text("");
        assert!(roots.is_empty());
    }

    #[test]
    fn measure_depth_works() {
        assert_eq!(measure_depth("Hello", 2), (0, "Hello"));
        assert_eq!(measure_depth("  World", 2), (1, "World"));
        assert_eq!(measure_depth("    Deep", 2), (2, "Deep"));
    }

    #[test]
    fn detect_indent_unit_works() {
        assert_eq!(detect_indent_unit(&["Hello", "  World"]), 2);
        assert_eq!(detect_indent_unit(&["Hello", "    World"]), 4);
        assert_eq!(detect_indent_unit(&["Hello"]), 2);
    }
}
