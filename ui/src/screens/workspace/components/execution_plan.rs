use crate::screens::workspace::{
    components::{ActionIcon, IconButton, send_sql_plan_request},
    context::WorkspaceAcpContext,
    tab_store::TabStore,
};
use dioxus::prelude::*;
use models::{ExecutionPlan, ExecutionPlanNode};
use std::collections::HashSet;

/// Color category for a plan node operation
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpCategory {
    Scan,
    Index,
    Join,
    Sort,
    Aggregate,
    Other,
}

fn classify_operation(op: &str) -> OpCategory {
    let lower = op.to_lowercase();
    if lower.contains("seq scan")
        || lower.contains("table scan")
        || lower.contains("scan table")
        || lower.contains("all")
        || lower.contains("readfrom")
    {
        OpCategory::Scan
    } else if lower.contains("index") {
        OpCategory::Index
    } else if lower.contains("join") || lower.contains("nested loop") {
        OpCategory::Join
    } else if lower.contains("sort") || lower.contains("order") {
        OpCategory::Sort
    } else if lower.contains("aggregate") || lower.contains("group") || lower.contains("hash") {
        OpCategory::Aggregate
    } else {
        OpCategory::Other
    }
}

/// Severity level for a plan analysis suggestion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdviceSeverity {
    Info,
    Warning,
    Critical,
}

impl AdviceSeverity {
    fn badge_class(self) -> &'static str {
        match self {
            AdviceSeverity::Info => "execution-plan__advice--info",
            AdviceSeverity::Warning => "execution-plan__advice--warning",
            AdviceSeverity::Critical => "execution-plan__advice--critical",
        }
    }
}

fn severity_label(severity: AdviceSeverity) -> &'static str {
    match severity {
        AdviceSeverity::Info => "Info",
        AdviceSeverity::Warning => "Warning",
        AdviceSeverity::Critical => "Critical",
    }
}

/// A single actionable suggestion produced by `analyze_plan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanAdvice {
    pub severity: AdviceSeverity,
    pub message: String,
}

/// Flatten every node in the plan tree (depth-first) for inspection.
fn collect_all_nodes(plan: &models::ExecutionPlan) -> Vec<&models::ExecutionPlanNode> {
    fn visit<'a>(
        nodes: &'a [models::ExecutionPlanNode],
        result: &mut Vec<&'a models::ExecutionPlanNode>,
    ) {
        for node in nodes {
            result.push(node);
            visit(&node.children, result);
        }
    }

    let mut result = Vec::new();
    visit(&plan.root_nodes, &mut result);
    result
}

/// Find the single node with the highest estimated cost, if any.
fn highest_cost_node(plan: &models::ExecutionPlan) -> Option<&models::ExecutionPlanNode> {
    collect_all_nodes(plan)
        .into_iter()
        .filter_map(|node| node.estimated_cost.map(|cost| (cost, node)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, node)| node)
}

/// True when a node is a Scan-category operation (seq scan / table scan).
fn is_scan_node(node: &models::ExecutionPlanNode) -> bool {
    classify_operation(&node.operation) == OpCategory::Scan
}

/// A leaf (childless) node directly under the given join that is itself a Scan.
/// Used to detect an unindexed inner relation in a nested loop join.
fn has_unindexed_scan_child(node: &models::ExecutionPlanNode) -> bool {
    node.children.iter().any(is_scan_node)
}

/// A pure, unit-testable analysis of an execution plan.
///
/// Returns actionable suggestions ordered Critical first, then Warning, then
/// Info. Only emits rules for which there is concrete evidence in the plan —
/// it never invents suggestions.
pub fn analyze_plan(plan: &models::ExecutionPlan) -> Vec<PlanAdvice> {
    const LARGE_TABLE_ROWS: u64 = 1000;
    const HIGH_COST_THRESHOLD: f64 = 1000.0;
    const ROW_ESTIMATE_MISMATCH_FACTOR: u64 = 5;

    let nodes = collect_all_nodes(plan);

    let mut critical: Vec<PlanAdvice> = Vec::new();
    let mut warnings: Vec<PlanAdvice> = Vec::new();
    let mut infos: Vec<PlanAdvice> = Vec::new();

    // Rule 1: Sequential scan on a large-ish table.
    for node in &nodes {
        if is_scan_node(node)
            && let Some(rows) = node.estimated_rows
            && rows >= LARGE_TABLE_ROWS
        {
            let target = node
                .target
                .clone()
                .unwrap_or_else(|| "the table".to_string());
            warnings.push(PlanAdvice {
                severity: AdviceSeverity::Warning,
                message: format!(
                    "Sequential scan on {target}; consider an index on the filter/join columns if the table is large."
                ),
            });
        }
    }

    // Rule 2: Nested loop join without an index on the inner side.
    for node in &nodes {
        let is_nested_loop = node.operation.to_lowercase().contains("nested loop");
        if classify_operation(&node.operation) == OpCategory::Join
            && is_nested_loop
            && has_unindexed_scan_child(node)
        {
            warnings.push(PlanAdvice {
                severity: AdviceSeverity::Warning,
                message: "Nested loop join — ensure the inner relation is indexed to avoid O(N\u{00d7}M) scans."
                    .to_string(),
            });
        }
    }

    // Rule 3: Sort without an index.
    for node in &nodes {
        if classify_operation(&node.operation) == OpCategory::Sort {
            infos.push(PlanAdvice {
                severity: AdviceSeverity::Info,
                message: "Sort node — an index on the ORDER BY columns would avoid a full sort."
                    .to_string(),
            });
        }
    }

    // Rule 4: very high-cost node (highest-cost leaf above threshold).
    if let Some(node) = highest_cost_node(plan)
        && let Some(cost) = node.estimated_cost
        && cost > HIGH_COST_THRESHOLD
    {
        let op = node.operation.clone();
        let target = node
            .target
            .clone()
            .unwrap_or_else(|| "the table".to_string());
        critical.push(PlanAdvice {
            severity: AdviceSeverity::Critical,
            message: format!("Highest-cost operation: {op} on {target} (cost {cost:.2})."),
        });
    }

    // Rule 5: ANALYZE row estimate mismatch.
    for node in &nodes {
        if let (Some(est), Some(actual)) = (node.estimated_rows, node.actual_rows)
            && actual > est.saturating_mul(ROW_ESTIMATE_MISMATCH_FACTOR)
        {
            critical.push(PlanAdvice {
                severity: AdviceSeverity::Critical,
                message: format!(
                    "Estimate mismatch: expected ~{est} rows but {actual} returned — stats may be stale; run ANALYZE."
                ),
            });
        }
    }

    // Rule 6: healthy plan when nothing was flagged.
    if critical.is_empty() && warnings.is_empty() {
        infos.push(PlanAdvice {
            severity: AdviceSeverity::Info,
            message: "Plan looks healthy — no obvious bottlenecks detected.".to_string(),
        });
    }

    critical.into_iter().chain(warnings).chain(infos).collect()
}

fn op_category_class(cat: OpCategory) -> &'static str {
    match cat {
        OpCategory::Scan => "execution-plan__node-badge--scan",
        OpCategory::Index => "execution-plan__node-badge--index",
        OpCategory::Join => "execution-plan__node-badge--join",
        OpCategory::Sort => "execution-plan__node-badge--sort",
        OpCategory::Aggregate => "execution-plan__node-badge--aggregate",
        OpCategory::Other => "execution-plan__node-badge--other",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanViewMode {
    Tree,
    Raw,
    Analysis,
}

type NodePath = Vec<usize>;

#[derive(Clone, Debug)]
struct VisiblePlanNode<'a> {
    node: &'a ExecutionPlanNode,
    path: NodePath,
    depth: usize,
    ancestor_has_more: Vec<bool>,
    is_last_sibling: bool,
    has_children: bool,
    is_expanded: bool,
}

fn collect_expandable_paths(nodes: &[ExecutionPlanNode]) -> HashSet<NodePath> {
    fn visit(nodes: &[ExecutionPlanNode], path: &mut Vec<usize>, result: &mut HashSet<NodePath>) {
        for (index, node) in nodes.iter().enumerate() {
            path.push(index);
            if !node.children.is_empty() {
                result.insert(path.clone());
                visit(&node.children, path, result);
            }
            path.pop();
        }
    }

    let mut result = HashSet::new();
    visit(nodes, &mut Vec::new(), &mut result);
    result
}

fn visible_plan_nodes<'a>(
    nodes: &'a [ExecutionPlanNode],
    expanded_paths: &HashSet<NodePath>,
) -> Vec<VisiblePlanNode<'a>> {
    fn visit<'a>(
        nodes: &'a [ExecutionPlanNode],
        expanded_paths: &HashSet<NodePath>,
        path: &mut Vec<usize>,
        ancestor_has_more: &mut Vec<bool>,
        depth: usize,
        result: &mut Vec<VisiblePlanNode<'a>>,
    ) {
        for (index, node) in nodes.iter().enumerate() {
            let is_last_sibling = index + 1 == nodes.len();
            path.push(index);
            let has_children = !node.children.is_empty();
            let is_expanded = has_children && expanded_paths.contains(path);

            result.push(VisiblePlanNode {
                node,
                path: path.clone(),
                depth,
                ancestor_has_more: ancestor_has_more.clone(),
                is_last_sibling,
                has_children,
                is_expanded,
            });

            if is_expanded {
                ancestor_has_more.push(!is_last_sibling);
                visit(
                    &node.children,
                    expanded_paths,
                    path,
                    ancestor_has_more,
                    depth + 1,
                    result,
                );
                ancestor_has_more.pop();
            }

            path.pop();
        }
    }

    let mut result = Vec::new();
    visit(
        nodes,
        expanded_paths,
        &mut Vec::new(),
        &mut Vec::new(),
        0,
        &mut result,
    );
    result
}

fn node_path_key(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::{AdviceSeverity, analyze_plan, collect_expandable_paths, visible_plan_nodes};
    use models::{ExecutionPlan, ExecutionPlanNode};
    use std::collections::HashSet;

    fn sample_plan_nodes() -> Vec<ExecutionPlanNode> {
        vec![
            ExecutionPlanNode::new("Root")
                .with_child(
                    ExecutionPlanNode::new("Child A").with_child(ExecutionPlanNode::new("Leaf")),
                )
                .with_child(ExecutionPlanNode::new("Child B")),
            ExecutionPlanNode::new("Other Root"),
        ]
    }

    #[test]
    fn visible_plan_nodes_hide_descendants_of_collapsed_nodes() {
        let nodes = sample_plan_nodes();
        let mut expanded = collect_expandable_paths(&nodes);
        expanded.remove(&vec![0]);

        let visible = visible_plan_nodes(&nodes, &expanded);
        let labels = visible
            .iter()
            .map(|entry| entry.node.operation.as_str())
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["Root", "Other Root"]);
    }

    #[test]
    fn visible_plan_nodes_keep_tree_metadata_for_connectors() {
        let nodes = sample_plan_nodes();
        let expanded = collect_expandable_paths(&nodes);

        let visible = visible_plan_nodes(&nodes, &expanded);
        let labels = visible
            .iter()
            .map(|entry| entry.node.operation.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec!["Root", "Child A", "Leaf", "Child B", "Other Root"]
        );
        assert_eq!(visible[2].path, vec![0, 0, 0]);
        assert_eq!(visible[2].depth, 2);
        assert_eq!(visible[2].ancestor_has_more, vec![true, true]);
        assert!(visible[3].is_last_sibling);
    }

    #[test]
    fn collect_expandable_paths_skips_leaf_nodes() {
        let nodes = sample_plan_nodes();
        let paths = collect_expandable_paths(&nodes);

        assert_eq!(paths, HashSet::from([vec![0], vec![0, 0]]));
    }

    fn plan_with(nodes: Vec<ExecutionPlanNode>) -> ExecutionPlan {
        let mut plan = ExecutionPlan::new("select * from t");
        plan.root_nodes = nodes;
        plan
    }

    #[test]
    fn analyze_flags_large_seq_scan_as_warning() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Seq Scan")
                .with_target("users")
                .with_rows(100_000),
        ]);

        let advice = analyze_plan(&plan);
        let seq = advice
            .iter()
            .find(|a| a.message.contains("Sequential scan on users"));
        assert!(seq.is_some(), "expected a seq-scan warning, got {advice:?}");
        assert_eq!(seq.unwrap().severity, AdviceSeverity::Warning);
    }

    #[test]
    fn analyze_ignores_small_seq_scan() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Seq Scan")
                .with_target("tiny")
                .with_rows(10),
        ]);

        let advice = analyze_plan(&plan);
        assert!(
            !advice.iter().any(|a| a.message.contains("Sequential scan")),
            "small seq scan should not be flagged: {advice:?}"
        );
    }

    #[test]
    fn analyze_flags_unindexed_nested_loop_as_warning() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Nested Loop").with_child(
                ExecutionPlanNode::new("Seq Scan")
                    .with_target("orders")
                    .with_rows(1000),
            ),
        ]);

        let advice = analyze_plan(&plan);
        let nested = advice
            .iter()
            .find(|a| a.message.contains("Nested loop join"));
        assert!(
            nested.is_some(),
            "expected nested-loop warning, got {advice:?}"
        );
        assert_eq!(nested.unwrap().severity, AdviceSeverity::Warning);
    }

    #[test]
    fn analyze_ignores_indexed_nested_loop() {
        let plan =
            plan_with(vec![ExecutionPlanNode::new("Nested Loop").with_child(
                ExecutionPlanNode::new("Index Scan").with_target("orders"),
            )]);

        let advice = analyze_plan(&plan);
        assert!(
            !advice
                .iter()
                .any(|a| a.message.contains("Nested loop join")),
            "indexed nested loop should not be flagged: {advice:?}"
        );
    }

    #[test]
    fn analyze_flags_sort_as_info() {
        let plan = plan_with(vec![ExecutionPlanNode::new("Sort")]);

        let advice = analyze_plan(&plan);
        let sort = advice.iter().find(|a| a.message.contains("Sort node"));
        assert!(sort.is_some(), "expected sort info, got {advice:?}");
        assert_eq!(sort.unwrap().severity, AdviceSeverity::Info);
    }

    #[test]
    fn analyze_flags_high_cost_node_as_critical() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Seq Scan")
                .with_target("huge")
                .with_cost(50_000.0)
                .with_rows(1_000_000),
        ]);

        let advice = analyze_plan(&plan);
        let high = advice
            .iter()
            .find(|a| a.message.contains("Highest-cost operation"));
        assert!(
            high.is_some(),
            "expected high-cost critical, got {advice:?}"
        );
        assert_eq!(high.unwrap().severity, AdviceSeverity::Critical);
    }

    fn scan_with_analyze_rows(estimated: u64, actual: u64) -> ExecutionPlanNode {
        ExecutionPlanNode {
            operation: "Index Scan".to_string(),
            target: Some("orders".to_string()),
            details: Vec::new(),
            children: Vec::new(),
            estimated_cost: None,
            estimated_rows: Some(estimated),
            actual_rows: Some(actual),
            actual_time_ms: None,
            raw_text: None,
        }
    }

    #[test]
    fn analyze_flags_row_estimate_mismatch_as_critical() {
        let plan = plan_with(vec![scan_with_analyze_rows(100, 60_000)]);

        let advice = analyze_plan(&plan);
        let mismatch = advice
            .iter()
            .find(|a| a.message.contains("Estimate mismatch"));
        assert!(
            mismatch.is_some(),
            "expected estimate-mismatch critical, got {advice:?}"
        );
        assert_eq!(mismatch.unwrap().severity, AdviceSeverity::Critical);
    }

    #[test]
    fn analyze_ignores_healthy_row_estimates() {
        let plan = plan_with(vec![scan_with_analyze_rows(100, 150)]);

        let advice = analyze_plan(&plan);
        assert!(
            !advice
                .iter()
                .any(|a| a.message.contains("Estimate mismatch")),
            "matching estimates should not be flagged: {advice:?}"
        );
    }

    #[test]
    fn analyze_reports_healthy_plan_as_info() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Index Scan")
                .with_target("orders")
                .with_rows(50),
        ]);

        let advice = analyze_plan(&plan);
        assert_eq!(
            advice.len(),
            1,
            "only the healthy line expected: {advice:?}"
        );
        assert_eq!(advice[0].severity, AdviceSeverity::Info);
        assert!(advice[0].message.contains("healthy"));
    }

    #[test]
    fn analyze_empty_plan_is_healthy_info() {
        let plan = plan_with(vec![]);
        let advice = analyze_plan(&plan);
        assert_eq!(
            advice.len(),
            1,
            "empty plan should still yield a line: {advice:?}"
        );
        assert_eq!(advice[0].severity, AdviceSeverity::Info);
    }

    #[test]
    fn analyze_orders_critical_before_warning_before_info() {
        let plan = plan_with(vec![
            ExecutionPlanNode::new("Nested Loop")
                .with_cost(5000.0)
                .with_child(ExecutionPlanNode::new("Seq Scan").with_rows(50)),
        ]);

        let advice = analyze_plan(&plan);
        let order: Vec<AdviceSeverity> = advice.iter().map(|a| a.severity).collect();
        assert_eq!(
            order,
            vec![AdviceSeverity::Critical, AdviceSeverity::Warning],
            "expected critical then warning, got {order:?}"
        );
    }
}

#[component]
pub fn ExecutionPlanView(plan: ExecutionPlan, store: TabStore) -> Element {
    let mut view_mode = use_signal(|| PlanViewMode::Tree);
    let mut expanded_nodes = use_signal(HashSet::<NodePath>::new);
    let mut expanded_plan_key = use_signal(String::new);

    let flattened = plan.flattened_with_depth();
    let raw_text = plan.raw_text.join("\n");
    let plan_advice = analyze_plan(&plan);
    let all_expandable_paths = collect_expandable_paths(&plan.root_nodes);
    let plan_state_key = format!(
        "{}\u{1f}{}\u{1f}{}",
        plan.explained_sql,
        flattened.len(),
        raw_text
    );
    let needs_reset = {
        let last_key = expanded_plan_key.peek();
        last_key.as_str() != plan_state_key.as_str()
    };
    let expanded_snapshot = if needs_reset {
        all_expandable_paths.clone()
    } else {
        expanded_nodes()
    };
    if needs_reset {
        expanded_plan_key.set(plan_state_key);
        expanded_nodes.set(expanded_snapshot.clone());
    }

    let node_count = flattened.len();
    let has_timing = plan.execution_time_ms.is_some() || plan.planning_time_ms.is_some();
    let visible_nodes = visible_plan_nodes(&plan.root_nodes, &expanded_snapshot);

    // Guarded with try_use_context: the plan view can render without an ACP
    // context (e.g. outside the workspace tree). When absent, hide the AI
    // button rather than crashing.
    let acp_ctx = try_use_context::<WorkspaceAcpContext>();
    let ai_button_visible = acp_ctx.is_some() && !plan.explained_sql.trim().is_empty();
    let ai_button_disabled = match acp_ctx.as_ref() {
        Some(ctx) => {
            let panel_busy = (ctx.acp_panel_state)().busy;
            let allow_read = (ctx.allow_agent_read_sql_run)();
            panel_busy || !allow_read
        }
        None => true,
    };

    rsx! {
        div { class: "execution-plan",
            // Header
            div { class: "execution-plan__header",
                div { class: "execution-plan__header-left",
                    span { class: "execution-plan__title",
                        "Execution Plan"
                    }
                    if plan.is_analyze {
                        span { class: "execution-plan__badge execution-plan__badge--analyze",
                            "ANALYZE"
                        }
                    }
                    span { class: "execution-plan__stat",
                        "{node_count} operations"
                    }
                }
                div { class: "execution-plan__header-right",
                    if ai_button_visible {
                        div { class: "execution-plan__ai-button",
                            IconButton {
                                icon: ActionIcon::Agent,
                                label: if ai_button_disabled {
                                    "Explain with AI (waiting for ACP)".to_string()
                                } else {
                                    "Explain with AI".to_string()
                                },
                                small: true,
                                disabled: ai_button_disabled,
                                onclick: move |_| {
                                    if let Some(ctx) = try_use_context::<WorkspaceAcpContext>() {
                                        let panel_state = ctx.acp_panel_state;
                                        let chat_revision = ctx.chat_revision;
                                        let allow_db_read = (ctx.allow_agent_db_read)();
                                        let allow_read_sql_run = (ctx.allow_agent_read_sql_run)();
                                        send_sql_plan_request(
                                            panel_state,
                                            store,
                                            store.active_tab_id(),
                                            ctx.connection_label.clone(),
                                            chat_revision,
                                            allow_db_read,
                                            allow_read_sql_run,
                                        );
                                    }
                                },
                            }
                        }
                    }
                    // View mode toggle
                    button {
                        class: if view_mode() == PlanViewMode::Tree {
                            "execution-plan__toggle execution-plan__toggle--active"
                        } else {
                            "execution-plan__toggle"
                        },
                        onclick: move |_| view_mode.set(PlanViewMode::Tree),
                        "Tree"
                    }
                    button {
                        class: if view_mode() == PlanViewMode::Raw {
                            "execution-plan__toggle execution-plan__toggle--active"
                        } else {
                            "execution-plan__toggle"
                        },
                        onclick: move |_| view_mode.set(PlanViewMode::Raw),
                        "Raw"
                    }
                    button {
                        class: if view_mode() == PlanViewMode::Analysis {
                            "execution-plan__toggle execution-plan__toggle--active"
                        } else {
                            "execution-plan__toggle"
                        },
                        onclick: move |_| view_mode.set(PlanViewMode::Analysis),
                        "Analysis"
                    }
                }
            }

            if view_mode() != PlanViewMode::Analysis {
                if !plan_advice.is_empty() {
                    div { class: "execution-plan__advice-strip",
                        for item in &plan_advice {
                            div {
                                class: "execution-plan__advice execution-plan__advice--compact {item.severity.badge_class()}",
                                span { class: "execution-plan__advice-badge",
                                    {severity_label(item.severity)}
                                }
                                span { class: "execution-plan__advice-text", "{item.message}" }
                            }
                        }
                    }
                }
            }

            // Summary stats
            if plan.total_cost.is_some() || has_timing {
                div { class: "execution-plan__stats",
                    if let Some(cost) = plan.total_cost {
                        div { class: "execution-plan__stat-chip",
                            span { class: "execution-plan__stat-label", "Total cost" }
                            span { class: "execution-plan__stat-value", "{cost:.2}" }
                        }
                    }
                    if let Some(pt) = plan.planning_time_ms {
                        div { class: "execution-plan__stat-chip",
                            span { class: "execution-plan__stat-label", "Planning" }
                            span { class: "execution-plan__stat-value", "{pt:.2} ms" }
                        }
                    }
                    if let Some(et) = plan.execution_time_ms {
                        div { class: "execution-plan__stat-chip",
                            span { class: "execution-plan__stat-label", "Execution" }
                            span { class: "execution-plan__stat-value", "{et:.2} ms" }
                        }
                    }
                }
            }

            // View content
            div { class: "execution-plan__content",
                match view_mode() {
                    PlanViewMode::Tree => rsx! {
                        div { class: "execution-plan__tree",
                            for node_view in &visible_nodes {
                                {
                                    let node = node_view.node;
                                    let node_path = node_view.path.clone();
                                    let node_key = node_path_key(&node_path);
                                    let is_expanded = node_view.is_expanded;
                                    let has_children = node_view.has_children;
                                    let cat = classify_operation(&node.operation);
                                    let badge_class = op_category_class(cat);
                                    let depth = node_view.depth;
                                    let ancestor_has_more = node_view.ancestor_has_more.clone();
                                    let is_last_sibling = node_view.is_last_sibling;
                                    let node_op = node.operation.clone();
                                    let node_target = node.target.clone();
                                    let node_details = node.details.clone();
                                    let node_cost = node.estimated_cost;
                                    let node_rows = node.estimated_rows;
                                    let node_actual_rows = node.actual_rows;
                                    let node_actual_time = node.actual_time_ms;
                                    let raw = node.raw_text.clone();

                                    rsx! {
                                        div {
                                            class: "execution-plan__node",
                                            key: "{node_key}",

                                            div { class: "execution-plan__tree-row",
                                                div { class: "execution-plan__guides",
                                                    for has_more in &ancestor_has_more {
                                                        span {
                                                            class: if *has_more {
                                                                "execution-plan__guide execution-plan__guide--continue"
                                                            } else {
                                                                "execution-plan__guide"
                                                            }
                                                        }
                                                    }
                                                    span {
                                                        class: if depth == 0 {
                                                            "execution-plan__guide execution-plan__guide--root"
                                                        } else if is_last_sibling {
                                                            "execution-plan__guide execution-plan__guide--branch-end"
                                                        } else {
                                                            "execution-plan__guide execution-plan__guide--branch-mid"
                                                        }
                                                    }
                                                }

                                                if has_children {
                                                    button {
                                                        class: "execution-plan__expand",
                                                        onclick: {
                                                            let mut expanded_nodes = expanded_nodes;
                                                            let path = node_path.clone();
                                                            move |_| {
                                                                if expanded_nodes().contains(&path) {
                                                                    expanded_nodes.write().remove(&path);
                                                                } else {
                                                                    expanded_nodes.write().insert(path.clone());
                                                                }
                                                            }
                                                        },
                                                        if is_expanded { "▼" } else { "▶" }
                                                    }
                                                } else {
                                                    span { class: "execution-plan__expand execution-plan__expand--leaf", "●" }
                                                }

                                                div { class: "execution-plan__node-content",
                                                    div { class: "execution-plan__node-header",
                                                        span { class: "execution-plan__node-badge {badge_class}",
                                                            "{node_op}"
                                                        }

                                                        if let Some(target) = &node_target {
                                                            span { class: "execution-plan__node-target",
                                                                "on {target}"
                                                            }
                                                        }
                                                    }

                                                    if node_cost.is_some() || node_rows.is_some() || node_actual_rows.is_some() || node_actual_time.is_some() {
                                                        div { class: "execution-plan__node-metrics",
                                                            if let Some(c) = node_cost {
                                                                span { class: "execution-plan__metric", "cost: {c:.2}" }
                                                            }
                                                            if let Some(r) = node_rows {
                                                                span { class: "execution-plan__metric", "rows: {r}" }
                                                            }
                                                            if let Some(r) = node_actual_rows {
                                                                span { class: "execution-plan__metric execution-plan__metric--actual",
                                                                    "actual: {r}"
                                                                }
                                                            }
                                                            if let Some(t) = node_actual_time {
                                                                span { class: "execution-plan__metric execution-plan__metric--actual",
                                                                    "time: {t:.2}ms"
                                                                }
                                                            }
                                                        }
                                                    }

                                                    if !node_details.is_empty() {
                                                        div { class: "execution-plan__node-details",
                                                            for (key, value) in &node_details {
                                                                span { class: "execution-plan__node-detail",
                                                                    "{key}: {value}"
                                                                }
                                                            }
                                                        }
                                                    }

                                                    if let Some(raw_text) = &raw {
                                                        if node_details.is_empty() && node_target.is_none() {
                                                            div { class: "execution-plan__node-details",
                                                                span { class: "execution-plan__node-raw",
                                                                    "{raw_text}"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    PlanViewMode::Raw => rsx! {
                        div { class: "execution-plan__raw",
                            div { class: "execution-plan__raw-sql",
                                span { class: "execution-plan__stat-label", "Query:" }
                                code { "{plan.explained_sql}" }
                            }
                            pre { class: "execution-plan__raw-text",
                                "{raw_text}"
                            }
                        }
                    },
                    PlanViewMode::Analysis => rsx! {
                        div { class: "execution-plan__analysis",
                            for item in &plan_advice {
                                div {
                                    class: "execution-plan__advice {item.severity.badge_class()}",
                                    span { class: "execution-plan__advice-badge",
                                        {severity_label(item.severity)}
                                    }
                                    span { class: "execution-plan__advice-text", "{item.message}" }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
