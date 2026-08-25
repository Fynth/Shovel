use dioxus::prelude::*;
use models::{AcpPanelState, ChatThreadSummary, QueryHistoryItem, SavedQuery};

use super::tab_store::TabStore;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct WorkspaceTabContext {
    pub store: TabStore,
    pub active_tab_id: Signal<u64>,
    pub next_tab_id: Signal<u64>,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct WorkspaceQueryContext {
    pub history: Signal<Vec<QueryHistoryItem>>,
    pub next_history_id: Signal<u64>,
    pub saved_queries: Signal<Vec<SavedQuery>>,
    pub next_saved_query_id: Signal<u64>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct WorkspaceAcpContext {
    pub acp_panel_state: Signal<AcpPanelState>,
    pub chat_revision: Signal<u64>,
    pub allow_agent_db_read: Signal<bool>,
    pub allow_agent_read_sql_run: Signal<bool>,
    pub allow_agent_write_sql_run: Signal<bool>,
    pub allow_agent_tool_run: Signal<bool>,
    pub chat_threads: Signal<Vec<ChatThreadSummary>>,
    pub active_chat_thread_id: Signal<Option<i64>>,
    pub connection_label: String,
}

pub fn provide_workspace_tab_context(
    store: TabStore,
    active_tab_id: Signal<u64>,
    next_tab_id: Signal<u64>,
) {
    provide_context(WorkspaceTabContext {
        store,
        active_tab_id,
        next_tab_id,
    });
}

pub fn provide_workspace_query_context(
    history: Signal<Vec<QueryHistoryItem>>,
    next_history_id: Signal<u64>,
    saved_queries: Signal<Vec<SavedQuery>>,
    next_saved_query_id: Signal<u64>,
) {
    provide_context(WorkspaceQueryContext {
        history,
        next_history_id,
        saved_queries,
        next_saved_query_id,
    });
}

pub fn provide_workspace_acp_context(context: WorkspaceAcpContext) {
    provide_context(context);
}
