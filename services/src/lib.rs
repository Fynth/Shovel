// Services facade — unified API for Shovel operations.
//
// This crate re-exports the public APIs from individual service crates
// to provide a single dependency point. Other crates (especially `ui`)
// can depend on `services` instead of importing from each crate individually.
//
// Crates covered:
// - connection — database connection management
// - explorer — schema exploration and table metadata
// - query — query execution, formatting, import/export, and table editing
// - storage — local persistence for settings, sessions, queries, and chat
// - acp — ACP agent runtime, registry, and context building

mod app;

// --- Connection management ---

pub use app::{
    AppStartupSettings,
    ConnectAndSaveResult,
    SessionRestoreResult,
    connect_and_save_request,
    load_app_startup_settings,
    restore_saved_sessions,
    save_app_ui_settings_with_secrets,
};
pub use connection::{connect_to_db, release_ssh_tunnel, test_connection};

// --- Schema exploration ---

pub use explorer::{
    describe_table,
    load_connection_tree,
    load_foreign_keys,
    load_object_ddl,
    load_table_columns,
};

// --- Query execution and table editing ---

pub use query::{
    BatchPlan,
    CommitKind,
    DatabaseFamily,
    Statement,
    StatementKind,
    StatementOutcome,
    create_table,
    delete_table_row,
    drop_table,
    duplicate_table,
    execute_explain,
    execute_query,
    execute_query_page,
    export_query_page_csv,
    export_query_page_html,
    export_query_page_json,
    export_query_page_sql_dump,
    export_query_page_xlsx,
    export_query_page_xml,
    format_insert_statements,
    format_sql,
    import_csv_into_table,
    insert_table_row,
    insert_table_row_with_values,
    is_read_only_sql,
    load_table_preview_page,
    next_table_primary_key_id,
    plan_batch,
    preview_source_for_sql,
    rename_table,
    split_sql,
    truncate_table,
    update_table_cell,
};

// --- Persistence ---

pub use storage::{
    QueryHistoryStore,
    acp_workspace_root,
    append_query_history,
    create_chat_thread,
    delete_chat_thread,
    delete_saved_query,
    load_app_ui_settings,
    load_chat_thread_messages,
    load_chat_threads,
    load_codestral_api_key,
    load_deepseek_api_key,
    load_query_history,
    load_saved_connections,
    load_saved_queries,
    load_session_state,
    load_session_state_sync,
    load_sql_format_settings,
    replace_connection_request,
    save_app_ui_settings,
    save_chat_thread_snapshot,
    save_codestral_api_key,
    save_connection_request,
    save_deepseek_api_key,
    save_saved_query,
    save_session_state,
    save_session_state_sync,
    save_sql_format_settings,
};

// --- ACP agent runtime ---

pub use acp::{
    build_acp_database_context,
    build_embedded_deepseek_launch,
    build_embedded_ollama_launch,
    cancel_acp_prompt,
    connect_acp_agent,
    disconnect_acp_agent,
    drain_acp_events,
    install_acp_registry_agent,
    load_acp_registry_agents,
    native_chat_prompt,
    record_execution,
    refresh_provider_models,
    respond_acp_permission,
    send_acp_prompt,
    send_acp_prompt_with_routing,
    warm_acp_database_schema_context,
};
