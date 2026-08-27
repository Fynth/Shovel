// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::ExplainExec;
use models::{DatabaseError, ExecutionPlan, ExecutionPlanNode};
use sqlx::Row;

use crate::session::PostgresSession;

#[async_trait]
impl ExplainExec for PostgresSession {
    async fn execute_explain(
        &self,
        sql: &str,
        analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        execute_postgres_explain(&self.pool, trimmed, analyze).await
    }
}

async fn execute_postgres_explain(
    pool: &sqlx::PgPool,
    sql: &str,
    analyze: bool,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = if analyze {
        format!("EXPLAIN (FORMAT JSON, VERBOSE, ANALYZE) {sql}")
    } else {
        format!("EXPLAIN (FORMAT JSON, VERBOSE) {sql}")
    };

    let rows = sqlx::query(&explain_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    // PostgreSQL returns the JSON as a single column in a single row.
    let mut raw_lines: Vec<String> = Vec::new();
    let mut json_text = String::new();

    for row in &rows {
        let value: String = row.try_get(0).unwrap_or_default();
        raw_lines.push(value.clone());
        json_text.push_str(&value);
    }

    let mut plan = ExecutionPlan::new(sql);
    plan.is_analyze = analyze;

    // Attempt JSON parsing.
    match serde_json::from_str::<Vec<serde_json::Value>>(&json_text) {
        Ok(plans) => {
            if let Some(first_plan) = plans.first()
                && let Some(plan_obj) = first_plan.as_object()
            {
                // Extract planning / execution time.
                plan.planning_time_ms = plan_obj.get("Planning Time").and_then(|v| v.as_f64());
                plan.execution_time_ms = plan_obj.get("Execution Time").and_then(|v| v.as_f64());

                if let Some(root_json) = plan_obj.get("Plan") {
                    let root_node = parse_postgres_plan_node(root_json);
                    plan.total_cost = root_json.get("Total Cost").and_then(|v| v.as_f64());
                    plan.root_nodes = vec![root_node];
                }
            }
        }
        Err(_) => {
            // JSON parse failed – fall back to raw text representation.
            plan.root_nodes = raw_lines
                .iter()
                .filter(|line| !line.trim().is_empty())
                .map(|line| ExecutionPlanNode::new("Raw").with_raw_text(line))
                .collect();
        }
    }

    plan.raw_text = raw_lines;
    Ok(plan)
}

/// Recursively parse a PostgreSQL JSON plan node.
///
/// Expected fields:
///   "Node Type": "Seq Scan" | "Index Scan" | "Hash Join" | etc.
///   "Relation Name": "users"
///   "Alias": "u"
///   "Startup Cost": 0.00
///   "Total Cost": 15.50
///   "Plan Rows": 550
///   "Plan Width": 68
///   "Plans": [ ... ]
///   "Filter": "..."
///   "Index Name": "..."
///   "Hash Cond": "..."
///   "Join Type": "..."
///   "Sort Key": [...]
///   "Group Key": [...]
///   "Actual Rows": 100  (ANALYZE)
///   "Actual Total Time": 1.234  (ANALYZE)
fn parse_postgres_plan_node(value: &serde_json::Value) -> ExecutionPlanNode {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return ExecutionPlanNode::new("Unknown").with_raw_text(value.to_string()),
    };

    let operation = obj
        .get("Node Type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let target = obj
        .get("Relation Name")
        .or_else(|| obj.get("Index Name"))
        .or_else(|| obj.get("Alias"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let cost = obj.get("Total Cost").and_then(|v| v.as_f64());
    let startup_cost = obj.get("Startup Cost").and_then(|v| v.as_f64());
    let plan_rows = obj.get("Plan Rows").and_then(|v| v.as_u64());
    let plan_width = obj.get("Plan Width").and_then(|v| v.as_u64());
    let actual_rows = obj.get("Actual Rows").and_then(|v| v.as_u64());
    let actual_time = obj.get("Actual Total Time").and_then(|v| v.as_f64());

    let mut node = ExecutionPlanNode::new(&operation);

    if let Some(target) = target {
        node = node.with_target(target);
    }
    if let Some(cost) = cost {
        node = node.with_cost(cost);
    }
    if let Some(rows) = plan_rows {
        node = node.with_rows(rows);
    }
    if let Some(rows) = actual_rows {
        node.actual_rows = Some(rows);
    }
    if let Some(time) = actual_time {
        node.actual_time_ms = Some(time);
    }

    // Add useful details.
    if let Some(startup) = startup_cost {
        node = node.with_detail("Startup Cost", format!("{startup:.2}"));
    }
    if let Some(width) = plan_width {
        node = node.with_detail("Plan Width", width.to_string());
    }
    if let Some(join_type) = obj.get("Join Type").and_then(|v| v.as_str()) {
        node = node.with_detail("Join Type", join_type);
    }
    if let Some(hash_cond) = obj.get("Hash Cond").and_then(|v| v.as_str()) {
        node = node.with_detail("Hash Cond", hash_cond);
    }
    if let Some(filter) = obj.get("Filter").and_then(|v| v.as_str()) {
        node = node.with_detail("Filter", filter);
    }
    if let Some(sort_key) = obj.get("Sort Key")
        && let Some(arr) = sort_key.as_array()
    {
        let keys: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !keys.is_empty() {
            node = node.with_detail("Sort Key", keys.join(", "));
        }
    }
    if let Some(group_key) = obj.get("Group Key")
        && let Some(arr) = group_key.as_array()
    {
        let keys: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !keys.is_empty() {
            node = node.with_detail("Group Key", keys.join(", "));
        }
    }
    if let Some(index_name) = obj.get("Index Name").and_then(|v| v.as_str()) {
        node = node.with_detail("Index Name", index_name);
    }
    if let Some(index_cond) = obj.get("Index Cond").and_then(|v| v.as_str()) {
        node = node.with_detail("Index Cond", index_cond);
    }

    // Recurse into child plans.
    if let Some(plans) = obj.get("Plans").and_then(|v| v.as_array()) {
        let children: Vec<ExecutionPlanNode> = plans.iter().map(parse_postgres_plan_node).collect();
        node.children = children;
    }

    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_json_parsing() {
        let json = serde_json::json!([{
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "users",
                "Alias": "users",
                "Startup Cost": 0.00,
                "Total Cost": 15.50,
                "Plan Rows": 550,
                "Plan Width": 68,
                "Plans": []
            },
            "Planning Time": 0.123,
            "Execution Time": 0.456
        }]);

        let plans: Vec<serde_json::Value> = serde_json::from_str(&json.to_string()).unwrap();
        let first = &plans[0];
        let plan_obj = first.as_object().unwrap();

        let root = parse_postgres_plan_node(plan_obj.get("Plan").unwrap());
        assert_eq!(root.operation, "Seq Scan");
        assert_eq!(root.target.as_deref(), Some("users"));
        assert_eq!(root.estimated_cost, Some(15.5));
        assert_eq!(root.estimated_rows, Some(550));
        assert!(root.children.is_empty());
    }

    #[test]
    fn postgres_json_nested_parsing() {
        let json = serde_json::json!([{
            "Plan": {
                "Node Type": "Hash Join",
                "Join Type": "Inner",
                "Hash Cond": "u.id = p.user_id",
                "Total Cost": 100.0,
                "Plan Rows": 1000,
                "Plans": [
                    {
                        "Node Type": "Seq Scan",
                        "Relation Name": "users",
                        "Alias": "u",
                        "Total Cost": 15.50,
                        "Plan Rows": 550
                    },
                    {
                        "Node Type": "Hash",
                        "Total Cost": 20.0,
                        "Plan Rows": 200,
                        "Plans": [
                            {
                                "Node Type": "Index Scan",
                                "Relation Name": "posts",
                                "Index Name": "idx_posts_user_id",
                                "Total Cost": 18.0,
                                "Plan Rows": 200
                            }
                        ]
                    }
                ]
            }
        }]);

        let plans: Vec<serde_json::Value> = serde_json::from_str(&json.to_string()).unwrap();
        let root = parse_postgres_plan_node(plans[0].get("Plan").unwrap());

        assert_eq!(root.operation, "Hash Join");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].operation, "Seq Scan");
        assert_eq!(root.children[1].operation, "Hash");
        assert_eq!(root.children[1].children.len(), 1);
        assert_eq!(root.children[1].children[0].operation, "Index Scan");
    }
}
