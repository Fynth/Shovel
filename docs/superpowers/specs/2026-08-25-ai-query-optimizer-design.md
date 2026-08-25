# AI Query Optimizer — Design

**Date:** 2026-08-25
**Status:** Draft (awaiting user review)
**Path:** Architectural (new subsystem layer over the existing EXPLAIN + ACP flow)

## Problem

Shovel already has an EXPLAIN feature and an "Explain with AI" button wired to the
ACP agent. Today the AI response is delivered as free text into the chat panel, so
the user must switch tabs to read it, there is no structured output, and there is no
one-click way to apply an improved query. Local heuristics (`analyze_plan`) and the
AI advice are two separate, unmerged sources.

## Goal

A single **Query Optimizer** panel inside the execution-plan view that merges local
heuristics and AI advice into structured recommendation cards, with an "apply"
button and a before/after comparison.

## Architecture

### New model types (`models`)

Add to `models/src/execution_plan.rs` (or a new `optimizer.rs` module):

```rust
pub struct QueryOptimizerResult {
    pub summary: String,
    pub recommendations: Vec<OptimizerRecommendation>,
    pub rewritten_sql: Option<String>,
}

pub struct OptimizerRecommendation {
    pub severity: AdviceSeverity,          // Critical / Warning / Info (reuse existing)
    pub category: RecommendationCategory, // Scan / Join / Sort / Index / Other
    pub title: String,
    pub detail: String,
    pub suggested_index: Option<String>,   // e.g. "CREATE INDEX idx_orders_user ON orders(user_id)"
}

pub enum RecommendationCategory { Scan, Join, Sort, Index, Other }
```

Add `optimizer_result: Option<QueryOptimizerResult>` to `QueryTabState`
(`models/src/query.rs`), defaulting to `None`.

### AI returns structured JSON

The ACP agent currently streams free text buffered into `hidden_agent_response`.
The prompt (`build_sql_plan_prompt`) is updated to ask the agent to return a single
fenced ```` ```json ```` block matching the `QueryOptimizerResult` shape. The UI
parses that block out of `hidden_agent_response` after the request completes.

### Data flow

```
User clicks "Optimize with AI" in the plan view
  -> send_sql_plan_request (existing) builds EXPLAIN + DB context
  -> AI returns JSON in hidden_agent_response
  -> UI parses JSON -> QueryOptimizerResult
  -> stored in tab.optimizer_result
  -> ExecutionPlanView renders cards + "Apply" button
```

## UI

A "Query Optimizer" section in `ExecutionPlanView`, below the plan tree (or as an
`Analysis` view mode). Contents:

- **"Optimize with AI"** button (visible when ACP is available; hidden otherwise).
- **Local heuristics** (existing `analyze_plan`) rendered as cards, badged "Local".
- **AI recommendations** rendered as cards, badged "AI", after the response lands.
- **Suggested index** rendered as a clickable `CREATE INDEX ...` snippet with a copy
  button (reuses `copy_text_to_clipboard`).
- **"Apply rewritten SQL"** button when `rewritten_sql` is present — inserts it into
  the active editor (reuses `insert_sql_into_editor`).
- **"Run EXPLAIN on optimized"** button — runs `run_explain_for_tab` on
  `rewritten_sql` to show a before/after plan.
- **Original / Optimized** toggle for comparing the two SQL texts.

## Error handling

1. **AI returns invalid JSON** — parser fails; show the raw text in a
   "AI returned unstructured response" card with a "view raw" affordance. Do not
   break the UI.
2. **AI returns JSON without `rewritten_sql`** — cards render; "Apply" is hidden.
3. **`rewritten_sql` is not read-only** — check `is_read_only_sql` before inserting;
   show a warning and do not insert (safety).
4. **ACP unavailable** — "Optimize with AI" hidden; local heuristics still work.
5. **EXPLAIN on optimized fails** — show the error in the tab status; leave the
   current plan untouched.

## Testing

- Unit tests for the JSON parser: valid, invalid, partial, with/without
  `rewritten_sql`.
- Unit tests for `is_read_only_sql` on `rewritten_sql`.
- Test that `QueryTabState` stores and clears `optimizer_result`.
- Manual: real EXPLAIN + AI response -> cards render, Apply inserts SQL.

## Out of scope (future sprints)

- Schema diff / synchronization
- Virtualized result grid
- Command palette
- Transaction sandbox / read-only mode
- Data lineage / impact analysis

## Scope estimate

Medium sprint (~7 tasks). Builds on the existing EXPLAIN + ACP flow; no new
infrastructure required.
