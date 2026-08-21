//! Local persistence layer for Shovel — settings, sessions, connections, query history, saved queries, and chat database.

mod chat;
mod fs_store;
mod history;
mod query_history;
mod saved_queries;
mod secrets;
mod semantic_cache;
mod settings;

/// Chat thread persistence and FTS5-powered full-text search.
///
/// These functions manage chat threads and their messages in the SQLite database
/// (`shovel.db`). Threads are associated with a database connection and contain
/// ordered sequences of [`AcpUiMessage`](models::AcpUiMessage) values.
pub use chat::{
    create_chat_thread, delete_chat_thread, load_chat_thread_messages, load_chat_threads,
    save_chat_thread_snapshot, search_chat_messages, search_chat_sql_artifacts,
};
/// Saved connections, session state, and query history orchestration.
///
/// These functions manage the lifecycle of saved database connections (including
/// secret storage via the system keyring), session state persistence (open tabs
/// and the active connection), and query history recording.
pub use history::{
    append_query_history, load_query_history, load_saved_connections, load_session_state,
    load_session_state_sync, replace_connection_request, save_connection_request,
    save_session_state, save_session_state_sync,
};
/// SQLite-backed query history store with FTS5 full-text search.
///
/// [`QueryHistoryStore`] persists executed SQL statements along with metadata
/// (duration, rows returned, outcome, connection info) and supports FTS5-based
/// search across historical queries.
pub use query_history::QueryHistoryStore;
/// JSON-file backed saved SQL queries.
///
/// These functions persist user-saved SQL queries to `saved_queries.json`.
/// Queries are organized by folder and sorted by folder name, title, and ID.
pub use saved_queries::{delete_saved_query, load_saved_queries, save_saved_query};
/// Embedding-based semantic cache for LLM responses.
///
/// [`SemanticCacheStore`] uses sqlite-vec to store embeddings and perform
/// vector similarity search, enabling cache lookup of semantically similar
/// queries. [`CacheStats`] reports aggregate cache metrics.
pub use semantic_cache::{CacheStats, SemanticCacheStore};
/// UI settings, SQL format settings, and ACP API key persistence.
///
/// These functions load and save application preferences (theme, panel
/// visibility, etc.), SQL formatting options, and ACP provider API keys
/// (CodeStral and DeepSeek). API keys are stored in the system keyring
/// with a fallback to the local secret store.
pub use settings::{
    load_app_ui_settings, load_codestral_api_key, load_deepseek_api_key, load_sql_format_settings,
    save_app_ui_settings, save_codestral_api_key, save_deepseek_api_key, save_sql_format_settings,
};

/// Returns the root directory for ACP workspace data, creating it if it doesn't exist.
///
/// ACP workspace directories store per-agent workspace files scoped under
/// `{data_local_dir}/shovel/acp/workspace/`.
///
/// # Errors
///
/// Returns an error string if the directory cannot be created.
pub fn acp_workspace_root() -> Result<std::path::PathBuf, String> {
    let path = fs_store::acp_workspace_root();
    std::fs::create_dir_all(&path)
        .map_err(|err| format!("failed to create ACP workspace {}: {err}", path.display()))?;
    Ok(path)
}

/// Returns the runtime directory for a specific ACP agent, creating it if it doesn't exist.
///
/// ACP agent runtime directories live under
/// `{data_local_dir}/shovel/acp/runtime/{agent_name}/` and can be used by agents to store
/// transient runtime state.
///
/// # Errors
///
/// Returns an error string if the directory cannot be created.
pub fn acp_agent_runtime_root(agent_name: &str) -> Result<std::path::PathBuf, String> {
    let path = fs_store::acp_agent_runtime_root(agent_name);
    std::fs::create_dir_all(&path).map_err(|err| {
        format!(
            "failed to create ACP runtime root {}: {err}",
            path.display()
        )
    })?;
    Ok(path)
}
