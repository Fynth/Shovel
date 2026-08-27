// async-trait boxes Result-returning futures and adds `#[must_use]`, which
// trips clippy::double_must_use on every exec-trait method.
#![allow(clippy::double_must_use)]

use async_trait::async_trait;
use database::ExplainExec;
use models::{DatabaseError, ExecutionPlan, ExecutionPlanNode};
use sqlx::Row;

use crate::session::MysqlSession;

#[async_trait]
impl ExplainExec for MysqlSession {
    async fn execute_explain(
        &self,
        sql: &str,
        _analyze: bool,
    ) -> Result<ExecutionPlan, DatabaseError> {
        let trimmed = sql.trim().trim_end_matches(';').trim();
        execute_mysql_explain(&self.pool, trimmed).await
    }
}

async fn execute_mysql_explain(
    pool: &sqlx::MySqlPool,
    sql: &str,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = format!("EXPLAIN FORMAT=JSON {sql}");
    let rows = sqlx::query(&explain_sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DatabaseError::Driver(e.to_string()))?;

    let mut raw_lines: Vec<String> = Vec::new();
    let mut json_text = String::new();

    for row in &rows {
        let value: String = row.try_get(0).unwrap_or_default();
        raw_lines.push(value.clone());
        json_text.push_str(&value);
    }

    let mut plan = ExecutionPlan::new(sql);

    // Attempt JSON parsing.
    match serde_json::from_str::<serde_json::Value>(&json_text) {
        Ok(root) => {
            if let Some(query_block) = root.get("query_block") {
                let node = parse_mysql_query_block(query_block);
                plan.root_nodes = vec![node];
            } else {
                // Unexpected structure – try to parse generically.
                let node = parse_mysql_value_generic(&root);
                plan.root_nodes = vec![node];
            }
        }
        Err(_) => {
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

/// Parse a MySQL `query_block` JSON object.
///
/// A query_block contains:
///   "select_id": 1,
///   "cost_info": { "query_cost": "1.00" },
///   "table": { ... }  or  "ordering_operation": { ... }  or  "grouping_operation": { ... }
///   "nested_loop": [ ... ]
fn parse_mysql_query_block(block: &serde_json::Value) -> ExecutionPlanNode {
    let mut node = ExecutionPlanNode::new("Query Block");

    if let Some(select_id) = block.get("select_id").and_then(|v| v.as_u64()) {
        node = node.with_detail("select_id", select_id.to_string());
    }

    if let Some(cost_info) = block.get("cost_info")
        && let Some(query_cost) = cost_info.get("query_cost").and_then(|v| v.as_str())
    {
        node = node.with_detail("query_cost", query_cost);
        if let Ok(cost) = query_cost.parse::<f64>() {
            node = node.with_cost(cost);
        }
    }

    // Parse direct table reference.
    if let Some(table) = block.get("table") {
        let table_node = parse_mysql_table(table);
        node.children.push(table_node);
    }

    // Parse ordering operation.
    if let Some(ordering) = block.get("ordering_operation") {
        let ordering_node = parse_mysql_ordering_operation(ordering);
        node.children.push(ordering_node);
    }

    // Parse grouping operation.
    if let Some(grouping) = block.get("grouping_operation") {
        let grouping_node = parse_mysql_grouping_operation(grouping);
        node.children.push(grouping_node);
    }

    // Parse nested loop (join structure).
    if let Some(nested_loop) = block.get("nested_loop")
        && let Some(items) = nested_loop.as_array()
    {
        for item in items {
            if let Some(qb) = item.get("query_block") {
                let child = parse_mysql_query_block(qb);
                node.children.push(child);
            } else if let Some(table) = item.get("table") {
                let child = parse_mysql_table(table);
                node.children.push(child);
            }
        }
    }

    // Parse "duplicates_removal" / "union" etc.
    if let Some(union_op) = block.get("union_result") {
        let union_node = ExecutionPlanNode::new("Union").with_raw_text(union_op.to_string());
        node.children.push(union_node);
    }

    node
}

/// Parse a MySQL table object.
///
///   "table_name": "users",
///   "access_type": "ALL" | "ref" | "range" | "const" | "eq_ref" | "index",
///   "rows_examined_per_scan": 100,
///   "rows_produced_per_join": 100,
///   "filtered": "100.00",
///   "cost_info": { ... },
///   "used_key_parts": [ ... ],
///   "key": "PRIMARY",
///   "possible_keys": [ ... ],
///   "attached_condition": "..."
fn parse_mysql_table(table: &serde_json::Value) -> ExecutionPlanNode {
    let obj = match table.as_object() {
        Some(o) => o,
        None => return ExecutionPlanNode::new("Table").with_raw_text(table.to_string()),
    };

    let access_type = obj
        .get("access_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let table_name = obj
        .get("table_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let operation = match access_type {
        "ALL" => "Table Scan",
        "index" => "Index Full Scan",
        "range" => "Index Range Scan",
        "ref" => "Index Ref Lookup",
        "eq_ref" => "Unique Index Lookup",
        "const" => "Const Row Read",
        "system" => "System Row Read",
        other => other,
    };

    let mut node = ExecutionPlanNode::new(operation).with_target(table_name);

    if let Some(inserted) = obj.get("insert") {
        node = node.with_detail("insert", inserted.to_string());
    }

    if let Some(rows_examined) = obj.get("rows_examined_per_scan").and_then(|v| v.as_u64()) {
        node = node.with_detail("rows_examined_per_scan", rows_examined.to_string());
        node.estimated_rows = Some(rows_examined);
    }

    if let Some(rows_produced) = obj.get("rows_produced_per_join").and_then(|v| v.as_u64()) {
        node = node.with_detail("rows_produced_per_join", rows_produced.to_string());
    }

    if let Some(filtered) = obj.get("filtered").and_then(|v| v.as_str()) {
        node = node.with_detail("filtered", filtered);
    }

    if let Some(key) = obj.get("key").and_then(|v| v.as_str()) {
        node = node.with_detail("key", key);
    }

    if let Some(possible_keys) = obj.get("possible_keys")
        && let Some(arr) = possible_keys.as_array()
    {
        let keys: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !keys.is_empty() {
            node = node.with_detail("possible_keys", keys.join(", "));
        }
    }

    if let Some(used_parts) = obj.get("used_key_parts")
        && let Some(arr) = used_parts.as_array()
    {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !parts.is_empty() {
            node = node.with_detail("used_key_parts", parts.join(", "));
        }
    }

    if let Some(condition) = obj.get("attached_condition").and_then(|v| v.as_str()) {
        node = node.with_detail("attached_condition", condition);
    }

    if let Some(cost_info) = obj.get("cost_info") {
        if let Some(read_cost) = cost_info.get("read_cost").and_then(|v| v.as_str()) {
            node = node.with_detail("read_cost", read_cost);
        }
        if let Some(eval_cost) = cost_info.get("eval_cost").and_then(|v| v.as_str()) {
            node = node.with_detail("eval_cost", eval_cost);
        }
        if let Some(prefix_cost) = cost_info.get("prefix_cost").and_then(|v| v.as_str()) {
            node = node.with_detail("prefix_cost", prefix_cost);
        }
    }

    node
}

/// Parse a MySQL ordering_operation object.
fn parse_mysql_ordering_operation(value: &serde_json::Value) -> ExecutionPlanNode {
    let mut node = ExecutionPlanNode::new("Ordering");

    if let Some(cost_info) = value.get("cost_info")
        && let Some(query_cost) = cost_info.get("query_cost").and_then(|v| v.as_str())
    {
        node = node.with_detail("cost", query_cost);
    }

    // An ordering_operation may contain a table or nested structures.
    if let Some(table) = value.get("table") {
        node.children.push(parse_mysql_table(table));
    }

    if let Some(nested_loop) = value.get("nested_loop")
        && let Some(items) = nested_loop.as_array()
    {
        for item in items {
            if let Some(table) = item.get("table") {
                node.children.push(parse_mysql_table(table));
            }
        }
    }

    node
}

/// Parse a MySQL grouping_operation object.
fn parse_mysql_grouping_operation(value: &serde_json::Value) -> ExecutionPlanNode {
    let mut node = ExecutionPlanNode::new("Grouping");

    if let Some(cost_info) = value.get("cost_info")
        && let Some(query_cost) = cost_info.get("query_cost").and_then(|v| v.as_str())
    {
        node = node.with_detail("cost", query_cost);
    }

    if let Some(table) = value.get("table") {
        node.children.push(parse_mysql_table(table));
    }

    if let Some(nested_loop) = value.get("nested_loop")
        && let Some(items) = nested_loop.as_array()
    {
        for item in items {
            if let Some(table) = item.get("table") {
                node.children.push(parse_mysql_table(table));
            }
        }
    }

    node
}

/// Generic fallback for unexpected MySQL JSON structures.
fn parse_mysql_value_generic(value: &serde_json::Value) -> ExecutionPlanNode {
    match value {
        serde_json::Value::Object(obj) => {
            let operation = obj
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string());
            ExecutionPlanNode::new(&operation).with_raw_text(value.to_string())
        }
        _ => ExecutionPlanNode::new("Raw").with_raw_text(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_json_parsing() {
        let json = serde_json::json!({
            "query_block": {
                "select_id": 1,
                "cost_info": { "query_cost": "1.00" },
                "table": {
                    "table_name": "users",
                    "access_type": "ALL",
                    "rows_examined_per_scan": 100,
                    "rows_produced_per_join": 100,
                    "filtered": "100.00"
                }
            }
        });

        let root = parse_mysql_query_block(json.get("query_block").unwrap());
        assert_eq!(root.operation, "Query Block");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].operation, "Table Scan");
        assert_eq!(root.children[0].target.as_deref(), Some("users"));
        assert_eq!(root.children[0].estimated_rows, Some(100));
    }
}
