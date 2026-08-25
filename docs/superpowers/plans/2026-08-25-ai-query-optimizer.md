# AI Query Optimizer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the existing "Explain with AI" flow into a structured Query Optimizer panel that merges local heuristics and AI advice into recommendation cards with an apply button and before/after comparison.

**Architecture:** The ACP agent returns a fenced ```` ```json ```` block matching a new `QueryOptimizerResult` model. The UI buffers the optimizer's response in a **dedicated field on `AcpPanelState`** (`optimizer_response`), decoupled from the shared `hidden_agent_response` buffer (which is coupled to auto-SQL-execution and must not run the optimizer's `rewritten_sql`). The `use_acp` loop parses that dedicated buffer, stores the result on `QueryTabState.optimizer_result`, and `ExecutionPlanView` renders it as cards alongside the existing local `analyze_plan` heuristics. Apply/EXPLAIN buttons reuse existing `insert_sql_into_editor` and `run_explain_for_tab`.

**Tech Stack:** Rust, Dioxus 0.7, serde/serde_json, existing ACP runtime.

**Spec:** `docs/superpowers/specs/2026-08-25-ai-query-optimizer-design.md`

## Global Constraints

- Dioxus 0.7 APIs only (`use_signal`, `use_effect`, `#[component]`). No `cx`/`Scope`/`use_state`.
- Never hold a signal read/write across an `.await` point. Drop the borrow before awaiting.
- `models` must not depend on `ui`. `AdviceSeverity` lives in `ui`; the new optimizer severity enum must live in `models` and be self-contained.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` must all pass.
- `rustfmt.toml` sets `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- `is_read_only_sql` is at `query/src/core/mod.rs:90` (re-exported as `services::is_read_only_sql`).

---

### Task 1: Optimizer model types in `models`

**Files:**
- Modify: `models/src/execution_plan.rs` (append new types)
- Modify: `models/src/query.rs:211-250` (add field to `QueryTabState`) and `models/src/query.rs:280-308` (default)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub struct QueryOptimizerResult { pub summary: String, pub recommendations: Vec<OptimizerRecommendation>, pub rewritten_sql: Option<String> }`
  - `pub struct OptimizerRecommendation { pub severity: OptimizerSeverity, pub category: RecommendationCategory, pub title: String, pub detail: String, pub suggested_index: Option<String> }`
  - `pub enum OptimizerSeverity { Info, Warning, Critical }` (serde rename to lowercase: `info`/`warning`/`critical`)
  - `pub enum RecommendationCategory { Scan, Join, Sort, Index, Other }` (serde rename to lowercase)
  - `QueryTabState.optimizer_result: Option<QueryOptimizerResult>` (default `None`)

- [ ] **Step 1: Write the failing test**

Add to `models/src/execution_plan.rs` a `#[cfg(test)] mod tests` (or extend existing) that round-trips the new types through serde_json:

```rust
#[cfg(test)]
mod optimizer_tests {
    use super::*;

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models optimizer_tests`
Expected: FAIL — `QueryOptimizerResult`, `OptimizerRecommendation`, `OptimizerSeverity`, `RecommendationCategory`, and `optimizer_result` are not defined.

- [ ] **Step 3: Write minimal implementation**

Append to `models/src/execution_plan.rs`:

```rust
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
```

In `models/src/query.rs`, add the field to `QueryTabState`:

```rust
    pub optimizer_result: Option<QueryOptimizerResult>,
```

and to the `Default` impl:

```rust
            optimizer_result: None,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p models optimizer_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/execution_plan.rs models/src/query.rs
git commit -m "feat(models): add QueryOptimizerResult model types"
```

---

### Task 2: JSON parser for the AI optimizer response

**Files:**
- Create: `ui/src/screens/workspace/components/execution_plan/optimizer_parse.rs`
- Modify: `ui/src/screens/workspace/components/execution_plan.rs` (module wiring, if it is a directory) — otherwise create `ui/src/screens/workspace/components/optimizer_parse.rs` and re-export.

**Interfaces:**
- Consumes: `models::QueryOptimizerResult`, `models::OptimizerRecommendation`, `models::OptimizerSeverity`, `models::RecommendationCategory`.
- Produces:
  - `pub fn parse_optimizer_result(raw: &str) -> Result<QueryOptimizerResult, String>`
  - `pub fn extract_json_block(raw: &str) -> Option<String>` (extracts the first fenced ```` ```json ```` block, or the whole string if it parses as JSON)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use models::{OptimizerRecommendation, OptimizerSeverity, RecommendationCategory};

    #[test]
    fn parses_fenced_json_block() {
        let raw = "Here is the analysis:\n```json\n{\"summary\":\"s\",\"recommendations\":[],\"rewritten_sql\":null}\n```\n";
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.summary, "s");
        assert!(result.recommendations.is_empty());
        assert!(result.rewritten_sql.is_none());
    }

    #[test]
    fn parses_plain_json_without_fence() {
        let raw = "{\"summary\":\"s\",\"recommendations\":[],\"rewritten_sql\":\"SELECT 1\"}";
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.rewritten_sql.as_deref(), Some("SELECT 1"));
    }

    #[test]
    fn rejects_invalid_json() {
        let raw = "this is not json at all";
        assert!(parse_optimizer_result(raw).is_err());
    }

    #[test]
    fn parses_recommendation_fields() {
        let raw = r#"{"summary":"s","recommendations":[{"severity":"critical","category":"join","title":"t","detail":"d","suggested_index":"CREATE INDEX i ON t(c)"}],"rewritten_sql":null}"#;
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.recommendations.len(), 1);
        let rec = &result.recommendations[0];
        assert_eq!(rec.severity, OptimizerSeverity::Critical);
        assert_eq!(rec.category, RecommendationCategory::Join);
        assert_eq!(rec.suggested_index.as_deref(), Some("CREATE INDEX i ON t(c)"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui optimizer_parse`
Expected: FAIL — module/function not found.

- [ ] **Step 3: Write minimal implementation**

```rust
use models::{QueryOptimizerResult, RecommendationCategory, OptimizerSeverity};

/// Extract the first fenced ```json ... ``` block from a raw agent response.
/// Falls back to the whole string when no fence is present.
pub fn extract_json_block(raw: &str) -> Option<String> {
    let start_marker = "```json";
    if let Some(start) = raw.find(start_marker) {
        let after = &raw[start + start_marker.len()..];
        if let Some(end) = after.find("```") {
            let block = after[..end].trim();
            if !block.is_empty() {
                return Some(block.to_string());
            }
        }
    }
    // No fence: try the whole string as JSON.
    if raw.trim().starts_with('{') {
        Some(raw.trim().to_string())
    } else {
        None
    }
}

/// Parse a raw agent response into a `QueryOptimizerResult`.
pub fn parse_optimizer_result(raw: &str) -> Result<QueryOptimizerResult, String> {
    let block = extract_json_block(raw)
        .ok_or_else(|| "No JSON block found in the agent response.".to_string())?;
    serde_json::from_str(&block).map_err(|err| format!("Invalid optimizer JSON: {err}"))
}
```

Wire the module: if `execution_plan.rs` is a single file, add `mod optimizer_parse;` at the top and `use optimizer_parse::parse_optimizer_result;` where needed. If it is a directory, add the file and `pub mod optimizer_parse;` in `mod.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui optimizer_parse`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/optimizer_parse.rs
git commit -m "feat(ui): add optimizer JSON parser"
```

---

### Task 3: Update the plan prompt to request structured JSON

**Files:**
- Modify: `ui/src/screens/workspace/components/agent_panel/prompt.rs:173-210` (`build_sql_plan_prompt`)

**Interfaces:**
- Consumes: existing `build_sql_plan_prompt` signature (unchanged).
- Produces: a prompt that instructs the agent to return a single fenced ```` ```json ```` block matching `QueryOptimizerResult`.

- [ ] **Step 1: Write the failing test**

Add a test asserting the prompt contains the JSON schema instruction and the ```` ```json ```` marker:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_prompt_requests_structured_json() {
        let prompt = build_sql_plan_prompt(
            "test-db",
            "SELECT * FROM orders",
            "EXPLAIN SELECT * FROM orders",
            "Seq Scan on orders",
            None,
            None,
            None,
        );
        assert!(prompt.contains("```json"));
        assert!(prompt.contains("rewritten_sql"));
        assert!(prompt.contains("suggested_index"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui plan_prompt_requests_structured_json`
Expected: FAIL — prompt does not contain the markers.

- [ ] **Step 3: Write minimal implementation**

Replace the trailing instruction string in `build_sql_plan_prompt` (currently the `prompt.push_str(&(response_language_directive() + "Explain what the plan is doing..."))` block) with:

```rust
    prompt.push_str(
        &(response_language_directive()
            + "Analyze the query plan and return your findings as a single fenced ```json block with EXACTLY this shape (no other text outside the block):\n\
```json\n\
{\n\
  \"summary\": \"one-paragraph plain-language explanation\",\n\
  \"recommendations\": [\n\
    {\n\
      \"severity\": \"critical|warning|info\",\n\
      \"category\": \"scan|join|sort|index|other\",\n\
      \"title\": \"short title\",\n\
      \"detail\": \"explanation\",\n\
      \"suggested_index\": \"CREATE INDEX ... or null\"\n\
    }\n\
  ],\n\
  \"rewritten_sql\": \"improved read-only SQL or null\"\n\
}\n\
```\n\
Rules: severity and category must be lowercase. Do not invent exact costs or row counts beyond what the plan shows. Do not add LIMIT/OFFSET/TOP/FETCH/SAMPLE/TABLESAMPLE unless the original SQL already uses one. If a better read-only rewrite is obvious, put it in rewritten_sql; otherwise null.\n"),
    );
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui plan_prompt_requests_structured_json`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/agent_panel/prompt.rs
git commit -m "feat(ui): request structured JSON from plan optimizer prompt"
```

---

### Task 4: Dedicated optimizer response buffer on ACP state

**Files:**
- Modify: `models/src/acp.rs:80-115` (`AcpPanelState` struct + `new()`)
- Modify: `ui/src/screens/workspace/components/agent_panel/state.rs` (`apply_acp_events` + `buffer_hidden_message`)
- Modify: `ui/src/screens/workspace/components/agent_panel/requests.rs:460-466` (`send_sql_plan_request`)

**Interfaces:**
- Consumes: `AcpPanelState` (defined in `models/src/acp.rs`).
- Produces:
  - `AcpPanelState.optimizer_response: String` (new field, default `""`)
  - `AcpPanelState.optimizer_request_active: bool` (new field, default `false`)
  - `buffer_optimizer_message(state, kind, text)` — appends `text` to `optimizer_response` when `kind == AcpMessageKind::Agent` and `optimizer_request_active` is true.

- [ ] **Step 1: Write the failing test**

Add to `state.rs` tests:

```rust
#[test]
fn optimizer_response_buffers_only_when_active() {
    let mut state = AcpPanelState::default();
    state.optimizer_request_active = true;
    buffer_optimizer_message(&mut state, AcpMessageKind::Agent, "{\"summary\":\"s\"");
    buffer_optimizer_message(&mut state, AcpMessageKind::Agent, ",\"recommendations\":[]}");
    assert_eq!(state.optimizer_response, "{\"summary\":\"s\",\"recommendations\":[]}");
    // When not active, nothing is buffered.
    state.optimizer_request_active = false;
    buffer_optimizer_message(&mut state, AcpMessageKind::Agent, "extra");
    assert_eq!(state.optimizer_response, "{\"summary\":\"s\",\"recommendations\":[]}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui optimizer_response_buffers_only_when_active`
Expected: FAIL — field/function not defined.

- [ ] **Step 3: Write minimal implementation**

In `models/src/acp.rs`, add the two fields to `AcpPanelState` and initialize them in `new()`:

```rust
    pub optimizer_response: String,
    pub optimizer_request_active: bool,
```

```rust
            optimizer_response: String::new(),
            optimizer_request_active: false,
```

In `ui/src/screens/workspace/components/agent_panel/state.rs`, add:

```rust
fn buffer_optimizer_message(state: &mut AcpPanelState, kind: AcpMessageKind, text: String) {
    if state.optimizer_request_active
        && matches!(kind, AcpMessageKind::Agent)
        && !text.is_empty()
    {
        state.optimizer_response.push_str(&text);
    }
}
```

In `apply_acp_events`, in the `AcpEvent::Message { kind, text }` arm, call `buffer_optimizer_message` in addition to the existing `buffer_hidden_message`/`push_or_append_message` logic. In the `AcpEvent::PromptFinished` and `AcpEvent::Error` arms, set `optimizer_request_active = false` (keep `optimizer_response` intact for the loop to read).

In `requests.rs:460-466` (`send_sql_plan_request`), set `state.optimizer_request_active = true` and `state.optimizer_response.clear()` instead of relying on `suppress_transcript`/`hidden_agent_response`:

```rust
    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.optimizer_request_active = true;
        state.optimizer_response.clear();
        state.status = "Running EXPLAIN for the active SQL...".to_string();
    });
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui optimizer_response_buffers_only_when_active`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/acp.rs ui/src/screens/workspace/components/agent_panel/state.rs ui/src/screens/workspace/components/agent_panel/requests.rs
git commit -m "feat(ui): dedicated optimizer response buffer on ACP state"
```

---

### Task 5: Parse the optimizer buffer and store it on the tab

**Files:**
- Modify: `ui/src/screens/workspace/hooks/use_acp.rs:220-330` (the event-poll loop)

**Interfaces:**
- Consumes: `parse_optimizer_result` (Task 2), `AcpPanelState.optimizer_response` (Task 4), `tabs: Signal<Vec<QueryTabState>>`, `active_tab_id: Signal<u64>`.
- Produces: writes `tab.optimizer_result = Some(result)` on the active tab when a valid optimizer JSON is detected.

- [ ] **Step 1: Write the failing test**

Add a unit test for a new pure helper `store_optimizer_result_on_tab` in `optimizer_parse.rs`:

```rust
#[test]
fn stores_result_on_active_tab() {
    use models::QueryTabState;
    let mut tabs = vec![QueryTabState::default()];
    let result = QueryOptimizerResult {
        summary: "s".to_string(),
        recommendations: vec![],
        rewritten_sql: None,
    };
    store_optimizer_result_on_tab(&mut tabs, 0, result.clone());
    assert_eq!(tabs[0].optimizer_result, Some(result));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui stores_result_on_active_tab`
Expected: FAIL — helper not defined.

- [ ] **Step 3: Write minimal implementation**

Add to `optimizer_parse.rs`:

```rust
use models::QueryTabState;

/// Store a parsed optimizer result on the tab with the given id, if present.
pub fn store_optimizer_result_on_tab(
    tabs: &mut [QueryTabState],
    tab_id: u64,
    result: QueryOptimizerResult,
) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) {
        tab.optimizer_result = Some(result);
    }
}
```

In `use_acp.rs`, inside the event-poll loop, after the existing `pending_hidden_agent_sql` / `pending_agent_sql` handling, add:

```rust
let optimizer_parsed = {
    let panel_state = acp_panel_state();
    if panel_state.optimizer_request_active && !panel_state.optimizer_response.is_empty() {
        parse_optimizer_result(&panel_state.optimizer_response).ok()
    } else {
        None
    }
};
if let Some(result) = optimizer_parsed {
    let target_id = active_tab_id();
    tabs.with_mut(|all_tabs| {
        store_optimizer_result_on_tab(all_tabs, target_id, result);
    });
    acp_panel_state.with_mut(|state| {
        state.optimizer_request_active = false;
        state.optimizer_response.clear();
    });
}
```

This is decoupled from `hidden_agent_response`, so the optimizer's `rewritten_sql` is never auto-executed by the existing SQL-extraction path.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui stores_result_on_active_tab`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/hooks/use_acp.rs ui/src/screens/workspace/components/optimizer_parse.rs
git commit -m "feat(ui): store parsed optimizer result on active tab"
```

---

### Task 6: Render the Query Optimizer panel in the plan view

**Files:**
- Modify: `ui/src/screens/workspace/components/execution_plan.rs` (the `ExecutionPlanView` component, around the header and below the tree)

**Interfaces:**
- Consumes: `tab.optimizer_result` (via `tabs`/`active_tab_id`), `analyze_plan` (existing local heuristics), `parse_optimizer_result` (Task 2).
- Produces: a rendered "Query Optimizer" section with Local + AI cards, an "Optimize with AI" button, and Apply/EXPLAIN buttons (wired in Task 6).

- [ ] **Step 1: Write the failing test**

Add a pure helper `render_optimizer_cards` (or a data-builder) that combines local `PlanAdvice` and AI `OptimizerRecommendation` into a unified list for rendering, and test it:

```rust
#[test]
fn merges_local_and_ai_advice() {
    let local = vec![PlanAdvice {
        severity: AdviceSeverity::Warning,
        message: "Seq scan on users".to_string(),
    }];
    let ai = vec![OptimizerRecommendation {
        severity: OptimizerSeverity::Critical,
        category: RecommendationCategory::Join,
        title: "Unindexed join".to_string(),
        detail: "d".to_string(),
        suggested_index: None,
    }];
    let merged = merge_optimizer_cards(&local, &ai);
    assert_eq!(merged.len(), 2);
    assert!(merged[0].is_ai);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui merge_optimizer_cards`
Expected: FAIL — helper not defined.

- [ ] **Step 3: Write minimal implementation**

Add a small struct and builder in `execution_plan.rs`:

```rust
/// A unified card for the optimizer panel, from either local heuristics or AI.
struct OptimizerCard {
    is_ai: bool,
    severity: OptimizerSeverity,
    title: String,
    detail: String,
    suggested_index: Option<String>,
}

fn merge_optimizer_cards(
    local: &[PlanAdvice],
    ai: &[OptimizerRecommendation],
) -> Vec<OptimizerCard> {
    let mut cards: Vec<OptimizerCard> = local
        .iter()
        .map(|advice| OptimizerCard {
            is_ai: false,
            severity: match advice.severity {
                AdviceSeverity::Info => OptimizerSeverity::Info,
                AdviceSeverity::Warning => OptimizerSeverity::Warning,
                AdviceSeverity::Critical => OptimizerSeverity::Critical,
            },
            title: advice.message.clone(),
            detail: String::new(),
            suggested_index: None,
        })
        .collect();
    cards.extend(ai.iter().map(|rec| OptimizerCard {
        is_ai: true,
        severity: rec.severity,
        title: rec.title.clone(),
        detail: rec.detail.clone(),
        suggested_index: rec.suggested_index.clone(),
    }));
    cards
}
```

In `ExecutionPlanView`, read the active tab's `optimizer_result` and render the section. Add a `use_signal` for the "Optimize with AI" busy state and a `use_signal` for the Original/Optimized toggle. Render the cards with severity badges and an "AI"/"Local" badge. The "Optimize with AI" button calls the existing `send_sql_plan_request` (same wiring as the current AI button at `execution_plan.rs:668`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui merge_optimizer_cards`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/components/execution_plan.rs
git commit -m "feat(ui): render Query Optimizer panel with merged cards"
```

---

### Task 7: Apply and EXPLAIN-on-optimized actions

**Files:**
- Modify: `ui/src/screens/workspace/components/execution_plan.rs` (button handlers)
- Modify: `ui/src/screens/workspace/actions.rs` (add `apply_optimized_sql` helper) or reuse existing

**Interfaces:**
- Consumes: `insert_sql_into_editor` (from `agent_panel/prompt.rs`), `run_explain_for_tab` (`actions.rs:827`), `services::is_read_only_sql`.
- Produces: `pub fn apply_optimized_sql(tabs, active_tab_id, sql) -> Result<(), String>` that validates read-only and inserts.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn apply_optimized_sql_rejects_write_statements() {
    let result = apply_optimized_sql_impl("DELETE FROM orders");
    assert!(result.is_err());
}

#[test]
fn apply_optimized_sql_accepts_read_only() {
    let result = apply_optimized_sql_impl("SELECT * FROM orders");
    assert!(result.is_ok());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui apply_optimized_sql`
Expected: FAIL — helper not defined.

- [ ] **Step 3: Write minimal implementation**

Add a pure validation helper in `actions.rs`:

```rust
/// Validate that a rewritten SQL is safe to insert (read-only).
pub fn apply_optimized_sql_impl(sql: &str) -> Result<(), String> {
    if services::is_read_only_sql(sql) {
        Ok(())
    } else {
        Err("The optimized SQL is not read-only; refusing to insert.".to_string())
    }
}
```

In `ExecutionPlanView`, wire the two buttons:
- **"Apply rewritten SQL"**: call `apply_optimized_sql_impl(&rewritten_sql)`; on `Ok`, call `insert_sql_into_editor(...)` with the rewritten SQL; on `Err`, show the message in the tab status.
- **"Run EXPLAIN on optimized"**: call `run_explain_for_tab(tabs, active_tab_id(), connection, rewritten_sql)`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p ui apply_optimized_sql`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/screens/workspace/actions.rs ui/src/screens/workspace/components/execution_plan.rs
git commit -m "feat(ui): apply optimized SQL and re-explain actions"
```

---

### Task 8: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Run formatting check**

Run: `cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --workspace`
Expected: all pass, 0 failures.

- [ ] **Step 4: Manual smoke test**

Run: `cargo run -p app --features desktop`
Expected: open a connection, run a query, open the execution plan, click "Optimize with AI" (with an ACP agent connected). Verify: cards render (Local + AI), "Apply rewritten SQL" inserts read-only SQL, "Run EXPLAIN on optimized" shows a new plan, invalid-JSON responses show the raw-text fallback card.

- [ ] **Step 5: Commit any leftover fixes**

```bash
git add -A
git commit -m "chore: finalize AI Query Optimizer"
```
