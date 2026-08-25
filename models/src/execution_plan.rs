use serde::{Deserialize, Serialize};

/// A single node in an execution plan tree.
/// Represents one operation (scan, join, sort, etc.) in the query plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPlanNode {
    /// The operation type (e.g., "Seq Scan", "Index Scan", "Hash Join", "Sort", "Table scan")
    pub operation: String,
    /// The target table or index this operation acts on (if applicable)
    pub target: Option<String>,
    /// Key-value detail pairs (e.g., "cost" -> "0.00..15.50", "rows" -> "100", "Filter" -> "id > 5")
    pub details: Vec<(String, String)>,
    /// Child operations (sub-operations in the plan tree)
    pub children: Vec<ExecutionPlanNode>,
    /// Estimated cost (if available)
    pub estimated_cost: Option<f64>,
    /// Estimated rows (if available)
    pub estimated_rows: Option<u64>,
    /// Actual rows (if available, from EXPLAIN ANALYZE)
    pub actual_rows: Option<u64>,
    /// Actual time in ms (if available, from EXPLAIN ANALYZE)
    pub actual_time_ms: Option<f64>,
    /// Raw text line from the database (fallback for unparsed lines)
    pub raw_text: Option<String>,
}

impl ExecutionPlanNode {
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            target: None,
            details: Vec::new(),
            children: Vec::new(),
            estimated_cost: None,
            estimated_rows: None,
            actual_rows: None,
            actual_time_ms: None,
            raw_text: None,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.estimated_cost = Some(cost);
        self
    }

    pub fn with_rows(mut self, rows: u64) -> Self {
        self.estimated_rows = Some(rows);
        self
    }

    pub fn with_child(mut self, child: ExecutionPlanNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_raw_text(mut self, text: impl Into<String>) -> Self {
        self.raw_text = Some(text.into());
        self
    }
}

/// The full execution plan result for a query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPlan {
    /// The root plan nodes (usually one, but some databases may return multiple)
    pub root_nodes: Vec<ExecutionPlanNode>,
    /// The raw text output from the database (original EXPLAIN output)
    pub raw_text: Vec<String>,
    /// The SQL that was explained
    pub explained_sql: String,
    /// Total estimated cost (if available, mainly PostgreSQL)
    pub total_cost: Option<f64>,
    /// Planning time in ms (if available)
    pub planning_time_ms: Option<f64>,
    /// Execution time in ms (if available, from EXPLAIN ANALYZE)
    pub execution_time_ms: Option<f64>,
    /// Whether this is an ANALYZE plan (with actual timing data)
    pub is_analyze: bool,
}

impl ExecutionPlan {
    pub fn new(explained_sql: impl Into<String>) -> Self {
        Self {
            root_nodes: Vec::new(),
            raw_text: Vec::new(),
            explained_sql: explained_sql.into(),
            total_cost: None,
            planning_time_ms: None,
            execution_time_ms: None,
            is_analyze: false,
        }
    }

    /// Get all nodes flattened in depth-first order with their depth
    pub fn flattened_with_depth(&self) -> Vec<(&ExecutionPlanNode, usize)> {
        let mut result = Vec::new();
        for node in &self.root_nodes {
            Self::flatten_node(node, 0, &mut result);
        }
        result
    }

    fn flatten_node<'a>(
        node: &'a ExecutionPlanNode,
        depth: usize,
        result: &mut Vec<(&'a ExecutionPlanNode, usize)>,
    ) {
        result.push((node, depth));
        for child in &node.children {
            Self::flatten_node(child, depth + 1, result);
        }
    }
}

/// Severity of an optimizer recommendation. Kept in `models` (not `ui`) so the
/// model layer stays self-contained; serialized lowercase for the AI JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OptimizerSeverity {
    Info,
    Warning,
    Critical,
}

/// Category of an optimizer recommendation, used for icon/badge selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecommendationCategory {
    Scan,
    Join,
    Sort,
    Index,
    Other,
}

/// A single structured recommendation produced by the AI optimizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptimizerRecommendation {
    pub severity: OptimizerSeverity,
    pub category: RecommendationCategory,
    pub title: String,
    pub detail: String,
    pub suggested_index: Option<String>,
}

/// The full structured result of an AI query-optimization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryOptimizerResult {
    pub summary: String,
    pub recommendations: Vec<OptimizerRecommendation>,
    pub rewritten_sql: Option<String>,
}

#[cfg(test)]
mod optimizer_tests {
    use super::*;
    use crate::QueryTabState;

    #[test]
    fn optimizer_result_round_trips_through_json() {
        let result = QueryOptimizerResult {
            summary: "Nested loop join is the bottleneck.".to_string(),
            recommendations: vec![OptimizerRecommendation {
                severity: OptimizerSeverity::Critical,
                category: RecommendationCategory::Join,
                title: "Unindexed inner join".to_string(),
                detail: "orders.user_id has no index.".to_string(),
                suggested_index: Some(
                    "CREATE INDEX idx_orders_user ON orders(user_id)".to_string(),
                ),
            }],
            rewritten_sql: Some("SELECT * FROM orders WHERE user_id = 1".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: QueryOptimizerResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn optimizer_result_defaults_to_none_on_tab() {
        let tab = QueryTabState::default();
        assert!(tab.optimizer_result.is_none());
    }
}
