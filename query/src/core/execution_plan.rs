use database::DatabaseDriver;
use driver_clickhouse::ClickHouseDriver;
use models::{DatabaseConnection, DatabaseError, ExecutionPlan, ExecutionPlanNode};
use sqlx::Row;

/// Execute an EXPLAIN query and return a parsed execution plan.
///
/// For SQLite, runs `EXPLAIN QUERY PLAN {sql}` and normalizes already-prefixed
/// `EXPLAIN ...` input to avoid generating nested EXPLAIN statements.
/// For PostgreSQL, runs `EXPLAIN (FORMAT JSON, VERBOSE{, ANALYZE}) {sql}`.
/// For MySQL, runs `EXPLAIN FORMAT=JSON {sql}`.
/// For ClickHouse, runs `EXPLAIN {sql}`.
pub async fn execute_explain(
    connection: DatabaseConnection,
    sql: &str,
    analyze: bool,
) -> Result<ExecutionPlan, DatabaseError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();

    match connection {
        DatabaseConnection::Sqlite(pool) => execute_sqlite_explain(&pool, trimmed).await,
        DatabaseConnection::Postgres(pool) => {
            execute_postgres_explain(&pool, trimmed, analyze).await
        }
        DatabaseConnection::MySql(pool) => execute_mysql_explain(&pool, trimmed).await,
        DatabaseConnection::ClickHouse(config) => {
            execute_clickhouse_explain(&config, trimmed).await
        }
    }
}

// ---------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------

async fn execute_sqlite_explain(
    pool: &sqlx::SqlitePool,
    sql: &str,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = sqlite_explain_query_plan_sql(sql);
    let rows = sqlx::query(&explain_sql)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::Sqlite)?;

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

// ---------------------------------------------------------------------------
// PostgreSQL
// ---------------------------------------------------------------------------

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
        .map_err(DatabaseError::Postgres)?;

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

// ---------------------------------------------------------------------------
// MySQL
// ---------------------------------------------------------------------------

async fn execute_mysql_explain(
    pool: &sqlx::MySqlPool,
    sql: &str,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = format!("EXPLAIN FORMAT=JSON {sql}");
    let rows = sqlx::query(&explain_sql)
        .fetch_all(pool)
        .await
        .map_err(DatabaseError::MySql)?;

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

// ---------------------------------------------------------------------------
// ClickHouse
// ---------------------------------------------------------------------------

async fn execute_clickhouse_explain(
    config: &models::ClickHouseFormData,
    sql: &str,
) -> Result<ExecutionPlan, DatabaseError> {
    let explain_sql = format!("EXPLAIN {sql}");
    let raw_text = ClickHouseDriver
        .execute_text_query(config, &explain_sql)
        .await?;

    let raw_lines: Vec<String> = raw_text.lines().map(String::from).collect();
    let root_nodes = parse_clickhouse_plan_text(&raw_text);

    let mut plan = ExecutionPlan::new(sql);
    plan.root_nodes = root_nodes;
    plan.raw_text = raw_lines;
    Ok(plan)
}

/// Parse ClickHouse EXPLAIN indented text into a plan tree.
///
/// ClickHouse returns lines like:
/// ```text
/// Expression
///   Filter
///     ReadFromMergeTree (default.table)
/// ```
///
/// Indentation is typically 2 or 4 spaces per level. We detect the indent unit
/// from the first indented line.
fn parse_clickhouse_plan_text(text: &str) -> Vec<ExecutionPlanNode> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Detect indent unit from the first line that has leading whitespace.
    let indent_unit = detect_indent_unit(&lines);

    // Collect all lines with their depths as a flat list.
    // The tree reconstruction is handled by build_tree_from_stack.
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

/// Detect the indent unit (number of spaces per level) from the lines.
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
    2 // Default to 2 spaces
}

/// Measure the depth and content of an indented line.
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

/// Parse a single ClickHouse EXPLAIN line into a node.
///
/// Examples:
///   "Expression"
///   "Expression (Projection)"
///   "Filter (is_active)"
///   "ReadFromMergeTree (default.users)"
///   "Sorting (Sorting by expression)"
///   "Aggregating"
///   "Limit"
fn parse_clickhouse_line(line: &str) -> ExecutionPlanNode {
    let line = line.trim();
    if line.is_empty() {
        return ExecutionPlanNode::new("unknown");
    }

    // Try to split "Operation (detail)".
    if let Some(paren_start) = line.find('(')
        && line.ends_with(')')
    {
        let operation = line[..paren_start].trim();
        let detail = &line[(paren_start + 1)..(line.len() - 1)];
        let mut node = ExecutionPlanNode::new(operation);

        // For ReadFromMergeTree, the parenthesized content is often the table.
        if operation.eq_ignore_ascii_case("ReadFromMergeTree")
            || operation.eq_ignore_ascii_case("ReadFromStorage")
        {
            node = node.with_target(detail);
        }

        return node.with_detail("detail", detail);
    }

    ExecutionPlanNode::new(line)
}

/// Build a tree from a flat list of (depth, node) pairs.
fn build_tree_from_stack(items: &[(usize, ExecutionPlanNode)]) -> Vec<ExecutionPlanNode> {
    if items.is_empty() {
        return Vec::new();
    }

    // We'll use an index-based approach: each item tracks its children indices.
    let n = items.len();
    let mut children_indices: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut parent_of: Vec<Option<usize>> = vec![None; n];

    for i in 0..n {
        let (depth_i, _) = &items[i];
        // Find the nearest previous item with smaller depth.
        for j in (0..i).rev() {
            let (depth_j, _) = &items[j];
            if *depth_j < *depth_i {
                parent_of[i] = Some(j);
                children_indices[j].push(i);
                break;
            }
        }
    }

    // Collect roots (items with no parent).
    let roots: Vec<usize> = (0..n).filter(|&i| parent_of[i].is_none()).collect();

    // Recursively build nodes.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();
        sqlx::query("create table users (id integer primary key, name text)")
            .execute(&pool)
            .await
            .unwrap();

        let plan = execute_explain(
            DatabaseConnection::Sqlite(pool),
            "EXPLAIN select * from users",
            false,
        )
        .await
        .unwrap();

        assert_eq!(plan.root_nodes.len(), 1);
        assert_eq!(plan.root_nodes[0].operation, "Scan");
        assert_eq!(plan.root_nodes[0].target.as_deref(), Some("users"));
    }

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
        assert_eq!(detect_indent_unit(&["Hello"]), 2); // default
    }
}
