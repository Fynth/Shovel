//! Smoke test for the `services` facade.
//!
//! The whole point of the `services` crate is to be the single import
//! point the UI uses for operational calls. If a re-export is missing,
//! the UI will fail to compile. This test does not exercise the actual
//! behavior of every function — that is the job of the underlying
//! crates' tests. It only verifies that the symbols listed in
//! `services::lib` are reachable through the facade.
//!
//! We deliberately avoid taking function pointers with explicit arity
//! (e.g. `let _: fn(_, _) -> _ = services::foo`) because that would make
//! this test brittle: any arity change would require updating this file
//! in lockstep with the facade. The simpler check below — taking a
//! reference to each symbol — keeps the test focused on what actually
//! matters: "is this name still re-exported?".

#![allow(dead_code)]

use std::any::type_name_of_val;

/// Every symbol the UI pulls from the `services` facade. If a future
/// refactor drops a re-export, the reference below fails to compile and
/// the breakage is attributed to the facade, not to a downstream caller.
#[test]
fn facade_surface_compiles() {
    // --- Connection management ---
    let _ = &services::release_ssh_tunnel;
    let _ = &services::connect_to_db;
    let _ = &services::register_session;
    let _ = &services::unregister_session;
    let _ = &services::session;

    // --- Schema exploration ---
    let _ = &services::describe_table;
    let _ = &services::load_connection_tree;
    let _ = &services::load_table_columns;

    // --- Query execution and table editing ---
    let _ = &services::create_table;
    let _ = &services::drop_table;
    let _ = &services::duplicate_table;
    let _ = &services::truncate_table;
    let _ = &services::delete_table_row;
    let _ = &services::insert_table_row;
    let _ = &services::insert_table_row_with_values;
    let _ = &services::update_table_cell;
    let _ = &services::execute_query_page;
    let _ = &services::execute_query;
    let _ = &services::execute_explain;
    let _ = &services::load_table_preview_page;
    let _ = &services::next_table_primary_key_id;
    let _ = &services::is_read_only_sql;
    let _ = &services::preview_source_for_sql;
    let _ = &services::format_sql;
    let _ = &services::format_sql_for_session;

    // --- Import / export ---
    let _ = &services::export_query_page_csv;
    let _ = &services::export_query_page_json;
    let _ = &services::export_query_page_xlsx;
    let _ = &services::export_query_page_xml;
    let _ = &services::export_query_page_html;
    let _ = &services::export_query_page_sql_dump;
    let _ = &services::import_csv_into_table;

    // --- ACP agent runtime ---
    let _: Option<services::CompleteRequest> = None;
    let _: Option<services::CompletionToken> = None;
    let _: Option<services::NativeChatMessage> = None;
    let _: Option<services::NativeChatRequest> = None;
    let _ = &services::complete_sql;
    let _ = &services::stream_native_completion;
    let _ = &services::build_acp_database_context;
    let _ = &services::build_embedded_deepseek_launch;
    let _ = &services::build_embedded_ollama_launch;
    let _ = &services::cancel_acp_prompt;
    let _ = &services::connect_acp_agent;
    let _ = &services::disconnect_acp_agent;
    let _ = &services::drain_acp_events;
    let _ = &services::install_acp_registry_agent;
    let _ = &services::load_acp_registry_agents;
    let _ = &services::native_chat_prompt;
    let _ = &services::record_execution;
    let _ = &services::refresh_provider_models;
    let _ = &services::respond_acp_permission;
    let _ = &services::send_acp_prompt;
    let _ = &services::send_acp_prompt_with_routing;
    let _ = &services::warm_acp_database_schema_context;
}

#[test]
fn facade_lists_expected_storage_reexports() {
    // A representative subset of the storage re-exports. If a name moves
    // out of the facade, the UI call sites that depend on it will fail
    // first; this test gives a clearer "facade" attribution for that
    // breakage.
    let _ = type_name_of_val(&services::QueryHistoryStore);
    let _ = &services::append_query_history;
    let _ = &services::create_chat_thread;
    let _ = &services::delete_chat_thread;
    let _ = &services::delete_saved_query;
    let _ = &services::load_app_ui_settings;
    let _ = &services::load_chat_thread_messages;
    let _ = &services::load_chat_threads;
    let _ = &services::load_codestral_api_key;
    let _ = &services::load_deepseek_api_key;
    let _ = &services::load_query_history;
    let _ = &services::load_saved_connections;
    let _ = &services::load_saved_queries;
    let _ = &services::load_session_state;
    let _ = &services::load_session_state_sync;
    let _ = &services::load_sql_format_settings;
    let _ = &services::replace_connection_request;
    let _ = &services::save_app_ui_settings;
    let _ = &services::save_chat_thread_snapshot;
    let _ = &services::save_codestral_api_key;
    let _ = &services::save_connection_request;
    let _ = &services::save_deepseek_api_key;
    let _ = &services::save_saved_query;
    let _ = &services::save_session_state;
    let _ = &services::save_session_state_sync;
    let _ = &services::save_sql_format_settings;
    let _ = &services::acp_workspace_root;
}

#[test]
fn facade_lists_expected_app_helpers() {
    // App-level facade helpers used by `ui::app` and the connect screen.
    // We only check that the function names exist. The struct types
    // (`AppStartupSettings`, `ConnectAndSaveResult`, `SessionRestoreResult`)
    // are not referenced here because they have no public constructors
    // and Rust will reject `&Type` for a type that requires fields.
    let _ = &services::connect_and_save_request;
    let _ = &services::load_app_startup_settings;
    let _ = &services::restore_saved_sessions;
    let _ = &services::save_app_ui_settings_with_secrets;
}
