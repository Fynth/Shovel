use dioxus::prelude::*;
use models::{AcpMessageKind, AcpPanelState};

use super::{
    AgentSqlExecutionMode,
    clickhouse::resolve_agent_sql_execution,
    prompt::{
        active_editor_error,
        active_editor_focus_source,
        active_editor_prompt_context,
        active_editor_session_id,
        active_editor_sql,
        build_chat_prompt,
        build_sql_error_fix_prompt,
        build_sql_explanation_prompt,
        build_sql_generation_prompt,
        build_sql_plan_prompt,
        build_thread_history_context,
        describe_query_output,
        insert_sql_into_editor,
        preferred_sql_target_tab_id,
    },
    state::push_message,
};

use crate::screens::workspace::{
    actions::{run_query_for_tab, tab_session_or_error},
    tab_store::TabStore,
};

fn build_routing_context(
    connection_label: &str,
    active_tab_context: Option<&str>,
    db_context: Option<&str>,
) -> String {
    let mut parts = vec![format!("Connection: {connection_label}")];
    if let Some(active_tab_context) = active_tab_context.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("Active editor context:\n{active_tab_context}"));
    }
    if let Some(db_context) = db_context.filter(|value| !value.trim().is_empty()) {
        parts.push(format!("Live database context:\n{db_context}"));
    }
    parts.join("\n\n")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn send_chat_prompt_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
    prompt: String,
    mut prompt_draft: Signal<String>,
) {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() || panel_state().busy {
        return;
    }

    let thread_history = build_thread_history_context(&panel_state().messages);
    let session_id = if allow_db_read {
        active_editor_session_id(store, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(store, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(store, active_tab_id)
    } else {
        None
    };
    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = if allow_db_read {
            "Preparing connected database context for the agent...".to_string()
        } else {
            "Preparing prompt for the agent...".to_string()
        };
    });

    spawn(async move {
        let (contextual_prompt, routing_context) = match session_id {
            Some(session_id) => {
                match services::build_acp_database_context(
                    session_id,
                    connection_label.clone(),
                    focus_source,
                )
                .await
                {
                    Ok(db_context) => (
                        build_chat_prompt(
                            &connection_label,
                            &prompt,
                            Some(db_context.clone()),
                            active_tab_context.clone(),
                            thread_history.clone(),
                        ),
                        build_routing_context(
                            &connection_label,
                            active_tab_context.as_deref(),
                            Some(&db_context),
                        ),
                    ),
                    Err(_) => (
                        build_chat_prompt(
                            &connection_label,
                            &prompt,
                            None,
                            active_tab_context.clone(),
                            thread_history.clone(),
                        ),
                        build_routing_context(
                            &connection_label,
                            active_tab_context.as_deref(),
                            None,
                        ),
                    ),
                }
            }
            None => (
                build_chat_prompt(
                    &connection_label,
                    &prompt,
                    None,
                    active_tab_context.clone(),
                    thread_history.clone(),
                ),
                build_routing_context(&connection_label, active_tab_context.as_deref(), None),
            ),
        };

        match services::send_acp_prompt_with_routing(contextual_prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    push_message(state, AcpMessageKind::User, prompt.clone());
                    state.prompt.clear();
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for agent response...".to_string();
                });
                prompt_draft.set(String::new());
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });
}

/// Send a "Describe this table with AI" request from the explorer
/// context menu. Reuses the chat prompt pipeline but with a
/// pre-built prompt that asks the agent to describe the specified
/// table's structure, purpose, and notable columns.
#[allow(clippy::too_many_arguments)]
pub(crate) fn send_describe_object_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
    qualified_name: String,
) {
    if panel_state().busy {
        return;
    }

    let prompt = format!(
        "Describe the table {qualified_name} in the active database. \
    Explain its purpose, key columns, relationships, and any notable design choices. \
    If you can see the schema context, use it. Keep the description concise."
    );

    let active_tab_id = store.active_tab_id();
    let session_id = if allow_db_read {
        active_editor_session_id(store, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(store, active_tab_id);

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Preparing table description request...".to_string();
    });

    spawn(async move {
        let (contextual_prompt, routing_context) = match session_id {
            Some(session_id) => {
                match services::build_acp_database_context(
                    session_id,
                    connection_label.clone(),
                    focus_source,
                )
                .await
                {
                    Ok(db_context) => (
                        build_chat_prompt(
                            &connection_label,
                            &prompt,
                            Some(db_context.clone()),
                            None,
                            None,
                        ),
                        build_routing_context(&connection_label, None, Some(&db_context)),
                    ),
                    Err(_) => (
                        build_chat_prompt(&connection_label, &prompt, None, None, None),
                        build_routing_context(&connection_label, None, None),
                    ),
                }
            }
            None => (
                build_chat_prompt(&connection_label, &prompt, None, None, None),
                build_routing_context(&connection_label, None, None),
            ),
        };

        match services::send_acp_prompt_with_routing(contextual_prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Describe: {qualified_name}"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for agent response...".to_string();
                });
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn send_sql_generation_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
    prompt: String,
    mut prompt_draft: Option<Signal<String>>,
    record_in_agent_panel: bool,
) {
    let request = prompt.trim().to_string();
    if request.is_empty() || panel_state().busy {
        return;
    }

    let session_id = if allow_db_read {
        active_editor_session_id(store, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(store, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(store, active_tab_id)
    } else {
        None
    };
    let thread_history = build_thread_history_context(&panel_state().messages);
    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = true;
        state.suppress_transcript = !record_in_agent_panel;
        state.hidden_agent_response.clear();
        state.status = if allow_db_read {
            "Preparing connected database context for the agent...".to_string()
        } else {
            "Preparing prompt for the agent...".to_string()
        };
    });

    spawn(async move {
        let (prompt, routing_context) = match session_id {
            Some(session_id) => {
                match services::build_acp_database_context(
                    session_id,
                    connection_label.clone(),
                    focus_source,
                )
                .await
                {
                    Ok(db_context) => (
                        build_sql_generation_prompt(
                            &connection_label,
                            &request,
                            Some(db_context.clone()),
                            active_tab_context.clone(),
                            thread_history.clone(),
                        ),
                        build_routing_context(
                            &connection_label,
                            active_tab_context.as_deref(),
                            Some(&db_context),
                        ),
                    ),
                    Err(_) => (
                        build_sql_generation_prompt(
                            &connection_label,
                            &request,
                            None,
                            active_tab_context.clone(),
                            thread_history.clone(),
                        ),
                        build_routing_context(
                            &connection_label,
                            active_tab_context.as_deref(),
                            None,
                        ),
                    ),
                }
            }
            None => (
                build_sql_generation_prompt(
                    &connection_label,
                    &request,
                    None,
                    active_tab_context.clone(),
                    thread_history.clone(),
                ),
                build_routing_context(&connection_label, active_tab_context.as_deref(), None),
            ),
        };

        match services::send_acp_prompt_with_routing(prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    if record_in_agent_panel {
                        push_message(
                            state,
                            AcpMessageKind::User,
                            format!("Generate SQL: {request}"),
                        );
                    }
                    state.prompt.clear();
                    state.busy = true;
                    state.pending_sql_insert = true;
                    state.status = "Waiting for agent SQL to insert into the editor...".to_string();
                });
                if let Some(prompt_draft) = prompt_draft.as_mut() {
                    prompt_draft.set(String::new());
                }
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    state.pending_sql_insert = false;
                    state.suppress_transcript = false;
                    state.hidden_agent_response.clear();
                    if record_in_agent_panel {
                        push_message(state, AcpMessageKind::Error, err);
                    }
                });
                chat_revision += 1;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn send_sql_plan_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
    allow_read_sql_run: bool,
) {
    let Some(active_sql) = active_editor_sql(store, active_tab_id) else {
        panel_state.with_mut(|state| {
            state.status = "There is no active SQL to explain with EXPLAIN.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "There is no active SQL to explain with EXPLAIN.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    };

    if panel_state().busy {
        return;
    }

    if !allow_read_sql_run {
        panel_state.with_mut(|state| {
            state.status = "Enable read-only SQL execution to run EXPLAIN.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "Enable read-only SQL execution to run EXPLAIN.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    }

    if !services::is_read_only_sql(&active_sql) {
        panel_state.with_mut(|state| {
            state.status = "Explain Plan is available only for read-only SQL.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "Explain Plan is available only for read-only SQL.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    }

    let Some(session_id) = active_editor_session_id(store, active_tab_id) else {
        panel_state.with_mut(|state| {
            state.status = "The active tab connection is not available.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "The active tab connection is not available.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    };

    let explain_sql = build_explain_sql(&active_sql);
    let focus_source = active_editor_focus_source(store, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(store, active_tab_id)
    } else {
        None
    };
    let thread_history = build_thread_history_context(&panel_state().messages);

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.optimizer_request_active = true;
        state.optimizer_response.clear();
        state.status = "Running EXPLAIN for the active SQL...".to_string();
    });

    spawn(async move {
        let plan_output =
            match services::execute_query_page(session_id, explain_sql.clone(), 100, 0, None, None)
                .await
            {
                Ok(output) => output,
                Err(err) => {
                    let error = format!("Explain plan error: {err}");
                    panel_state.with_mut(|state| {
                        state.status = error.clone();
                        state.busy = false;
                        push_message(state, AcpMessageKind::Error, error);
                    });
                    chat_revision += 1;
                    return;
                }
            };
        let explain_plan = describe_query_output("Explain plan result", &plan_output);

        let (prompt, routing_context) = if allow_db_read {
            match services::build_acp_database_context(
                session_id,
                connection_label.clone(),
                focus_source,
            )
            .await
            {
                Ok(db_context) => (
                    build_sql_plan_prompt(
                        &connection_label,
                        &active_sql,
                        &explain_sql,
                        &explain_plan,
                        Some(db_context.clone()),
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(
                        &connection_label,
                        active_tab_context.as_deref(),
                        Some(&db_context),
                    ),
                ),
                Err(_) => (
                    build_sql_plan_prompt(
                        &connection_label,
                        &active_sql,
                        &explain_sql,
                        &explain_plan,
                        None,
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(&connection_label, active_tab_context.as_deref(), None),
                ),
            }
        } else {
            (
                build_sql_plan_prompt(
                    &connection_label,
                    &active_sql,
                    &explain_sql,
                    &explain_plan,
                    None,
                    active_tab_context.clone(),
                    thread_history.clone(),
                ),
                build_routing_context(&connection_label, active_tab_context.as_deref(), None),
            )
        };

        match services::send_acp_prompt_with_routing(prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Explain query plan:\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for query plan explanation...".to_string();
                });
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn send_sql_explanation_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
) {
    let Some(active_sql) = active_editor_sql(store, active_tab_id) else {
        panel_state.with_mut(|state| {
            state.status = "There is no active SQL to explain.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "There is no active SQL to explain.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    };

    if panel_state().busy {
        return;
    }

    let thread_history = build_thread_history_context(&panel_state().messages);
    let session_id = if allow_db_read {
        active_editor_session_id(store, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(store, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(store, active_tab_id)
    } else {
        None
    };

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Preparing active SQL for explanation...".to_string();
    });

    spawn(async move {
        let (prompt, routing_context) = match session_id {
            Some(session_id) => match services::build_acp_database_context(
                session_id,
                connection_label.clone(),
                focus_source,
            )
            .await
            {
                Ok(db_context) => (
                    build_sql_explanation_prompt(
                        &connection_label,
                        &active_sql,
                        Some(db_context.clone()),
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(
                        &connection_label,
                        active_tab_context.as_deref(),
                        Some(&db_context),
                    ),
                ),
                Err(_) => (
                    build_sql_explanation_prompt(
                        &connection_label,
                        &active_sql,
                        None,
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(&connection_label, active_tab_context.as_deref(), None),
                ),
            },
            None => (
                build_sql_explanation_prompt(
                    &connection_label,
                    &active_sql,
                    None,
                    active_tab_context.clone(),
                    thread_history.clone(),
                ),
                build_routing_context(&connection_label, active_tab_context.as_deref(), None),
            ),
        };

        match services::send_acp_prompt_with_routing(prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Explain active SQL:\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for SQL explanation...".to_string();
                });
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn send_sql_error_fix_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
) {
    let Some(active_sql) = active_editor_sql(store, active_tab_id) else {
        panel_state.with_mut(|state| {
            state.status = "There is no active SQL to repair.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "There is no active SQL to repair.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    };
    let Some(error) = active_editor_error(store, active_tab_id) else {
        panel_state.with_mut(|state| {
            state.status = "The active tab has no SQL error to fix.".to_string();
            push_message(
                state,
                AcpMessageKind::Error,
                "The active tab has no SQL error to fix.".to_string(),
            );
        });
        chat_revision += 1;
        return;
    };

    if panel_state().busy {
        return;
    }

    let session_id = if allow_db_read {
        active_editor_session_id(store, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(store, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(store, active_tab_id)
    } else {
        None
    };
    let thread_history = build_thread_history_context(&panel_state().messages);

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = true;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Preparing SQL repair prompt for the agent...".to_string();
    });

    spawn(async move {
        let (prompt, routing_context) = match session_id {
            Some(session_id) => match services::build_acp_database_context(
                session_id,
                connection_label.clone(),
                focus_source,
            )
            .await
            {
                Ok(db_context) => (
                    build_sql_error_fix_prompt(
                        &connection_label,
                        &active_sql,
                        &error,
                        Some(db_context.clone()),
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(
                        &connection_label,
                        active_tab_context.as_deref(),
                        Some(&db_context),
                    ),
                ),
                Err(_) => (
                    build_sql_error_fix_prompt(
                        &connection_label,
                        &active_sql,
                        &error,
                        None,
                        active_tab_context.clone(),
                        thread_history.clone(),
                    ),
                    build_routing_context(&connection_label, active_tab_context.as_deref(), None),
                ),
            },
            None => (
                build_sql_error_fix_prompt(
                    &connection_label,
                    &active_sql,
                    &error,
                    None,
                    active_tab_context.clone(),
                    thread_history.clone(),
                ),
                build_routing_context(&connection_label, active_tab_context.as_deref(), None),
            ),
        };

        match services::send_acp_prompt_with_routing(prompt, routing_context) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Fix SQL error: {error}\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = true;
                    state.status = "Waiting for repaired SQL...".to_string();
                });
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    state.busy = false;
                    state.pending_sql_insert = false;
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_agent_sql_request(
    mut panel_state: Signal<AcpPanelState>,
    mut store: TabStore,
    active_tab_id: Signal<u64>,
    mut chat_revision: Signal<u64>,
    sql: String,
    execution_mode: AgentSqlExecutionMode,
    record_error_in_agent_panel: bool,
) {
    // The auto-read-only path is the agent's "run read-only SQL" tool. It
    // MUST NOT execute a write statement regardless of what the caller
    // believes it passed, so gate here at the execution boundary (not just
    // in the button/auto-dispatch that asks for it).
    if let Some(message) = read_only_agent_sql_blocked(&sql, execution_mode) {
        panel_state.with_mut(|state| {
            state.status = message.clone();
            if record_error_in_agent_panel {
                push_message(state, AcpMessageKind::Error, message);
            }
        });
        chat_revision += 1;
        return;
    }

    let Some(target_tab_id) = preferred_sql_target_tab_id(store, active_tab_id()) else {
        panel_state.with_mut(|state| {
            state.status = "No active SQL tab to execute in.".to_string();
            if record_error_in_agent_panel {
                push_message(
                    state,
                    AcpMessageKind::Error,
                    "No active SQL tab to execute in.".to_string(),
                );
            }
        });
        chat_revision += 1;
        return;
    };

    let current_tab = store.result.read().get(&target_tab_id).cloned();
    let Some(current_tab) = current_tab else {
        panel_state.with_mut(|state| {
            state.status = "Active SQL tab was not found.".to_string();
            if record_error_in_agent_panel {
                push_message(
                    state,
                    AcpMessageKind::Error,
                    "Active SQL tab was not found.".to_string(),
                );
            }
        });
        chat_revision += 1;
        return;
    };

    let Some(meta) = store.meta.read().get(&target_tab_id).cloned() else {
        panel_state.with_mut(|state| {
            state.status = "Active SQL tab was not found.".to_string();
            if record_error_in_agent_panel {
                push_message(
                    state,
                    AcpMessageKind::Error,
                    "Active SQL tab was not found.".to_string(),
                );
            }
        });
        chat_revision += 1;
        return;
    };

    let Some(session_id) = tab_session_or_error(store, target_tab_id, meta.session_id) else {
        panel_state.with_mut(|state| {
            state.status = "The active tab connection is not available.".to_string();
            if record_error_in_agent_panel {
                push_message(
                    state,
                    AcpMessageKind::Error,
                    "The active tab connection is not available.".to_string(),
                );
            }
        });
        chat_revision += 1;
        return;
    };

    let base_status = match execution_mode {
        AgentSqlExecutionMode::Manual => "Executed agent SQL in the active SQL tab.".to_string(),
        AgentSqlExecutionMode::AutoReadOnly =>
            "Executed read-only SQL from the ACP agent.".to_string(),
    };

    spawn(async move {
        let resolved = match resolve_agent_sql_execution(session_id, &sql).await {
            Ok(resolved) => resolved,
            Err(err) => {
                store.result.with_mut(|m| {
                    if let Some(tab) = m.get_mut(&target_tab_id) {
                        tab.status = format!("Error: {err}");
                    }
                });
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    if record_error_in_agent_panel {
                        push_message(state, AcpMessageKind::Error, err);
                    }
                });
                chat_revision += 1;
                return;
            }
        };

        insert_sql_into_editor(panel_state, store, active_tab_id, resolved.sql.clone());

        panel_state.with_mut(|state| {
            state.status = match &resolved.correction_note {
                Some(note) => format!("{base_status} {note}"),
                None => base_status.clone(),
            };
        });
        let execution_mode_label = match execution_mode {
            AgentSqlExecutionMode::Manual => "manual",
            AgentSqlExecutionMode::AutoReadOnly => "auto-read-only",
        };
        let _ = services::record_execution(format!(
            "Executed agent SQL in tab '{}' ({execution_mode_label})",
            meta.title,
        ));
        chat_revision += 1;

        run_query_for_tab(
            store,
            target_tab_id,
            session_id,
            resolved.sql,
            0,
            current_tab.page_size,
            None,
        );
    });
}

pub(super) fn build_explain_sql(active_sql: &str) -> String {
    let trimmed = active_sql.trim();
    if trimmed
        .split_whitespace()
        .next()
        .is_some_and(|keyword| keyword.eq_ignore_ascii_case("explain"))
    {
        trimmed.to_string()
    } else {
        format!("EXPLAIN {trimmed}")
    }
}

pub(super) fn can_execute_agent_sql(
    sql: &str,
    allow_read_sql_run: bool,
    allow_write_sql_run: bool,
) -> bool {
    if services::is_read_only_sql(sql) {
        allow_read_sql_run
    } else {
        allow_write_sql_run
    }
}

/// Returns an error message when the auto-read-only agent SQL path should
/// be blocked, or `None` when the request may proceed. The read-only agent
/// tool must reject any write statement before it reaches the query
/// executor, regardless of the caller's intent.
pub(super) fn read_only_agent_sql_blocked(
    sql: &str,
    execution_mode: AgentSqlExecutionMode,
) -> Option<String> {
    if execution_mode == AgentSqlExecutionMode::AutoReadOnly && !services::is_read_only_sql(sql) {
        Some(format!(
            "Refusing to run write SQL through the read-only agent tool: {sql}"
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentSqlExecutionMode, build_explain_sql, read_only_agent_sql_blocked};

    #[test]
    fn prefixes_explain_for_regular_sql() {
        assert_eq!(
            build_explain_sql("select * from products"),
            "EXPLAIN select * from products"
        );
    }

    #[test]
    fn preserves_existing_explain_statement() {
        assert_eq!(
            build_explain_sql("EXPLAIN select * from products"),
            "EXPLAIN select * from products"
        );
    }

    #[test]
    fn read_only_tool_allows_read_statements() {
        assert_eq!(
            read_only_agent_sql_blocked(
                "select * from products",
                AgentSqlExecutionMode::AutoReadOnly
            ),
            None
        );
        assert_eq!(
            read_only_agent_sql_blocked(
                "WITH recent AS (SELECT 1) SELECT * FROM recent",
                AgentSqlExecutionMode::AutoReadOnly
            ),
            None
        );
    }

    #[test]
    fn read_only_tool_rejects_write_statements() {
        for sql in [
            "INSERT INTO products (id) VALUES (1)",
            "UPDATE products SET price = 0",
            "DELETE FROM products",
            "DROP TABLE products",
            "CREATE TABLE products (id INTEGER)",
        ] {
            assert!(
                read_only_agent_sql_blocked(sql, AgentSqlExecutionMode::AutoReadOnly).is_some(),
                "expected write SQL to be rejected by the read-only agent tool: {sql}"
            );
        }
    }

    #[test]
    fn manual_mode_is_not_gated_by_read_only_agent_tool() {
        assert_eq!(
            read_only_agent_sql_blocked("DELETE FROM products", AgentSqlExecutionMode::Manual),
            None
        );
    }
}
