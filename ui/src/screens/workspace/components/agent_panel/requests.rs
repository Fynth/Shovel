use dioxus::prelude::*;
use models::{AcpMessageKind, AcpPanelState, QueryTabState};

use super::{
    AgentSqlExecutionMode,
    clickhouse::resolve_agent_sql_execution,
    prompt::{
        active_editor_connection,
        active_editor_error,
        active_editor_focus_source,
        active_editor_prompt_context,
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

use crate::screens::workspace::actions::{run_query_for_tab, tab_connection_or_error};

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
    tabs: Signal<Vec<QueryTabState>>,
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
    let connection = if allow_db_read {
        active_editor_connection(tabs, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(tabs, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(tabs, active_tab_id)
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
        let (contextual_prompt, routing_context) = match connection {
            Some(connection) => {
                match services::build_acp_database_context(
                    connection,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
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

    let connection = if allow_db_read {
        active_editor_connection(tabs, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(tabs, active_tab_id);

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Preparing table description request...".to_string();
    });

    spawn(async move {
        let (contextual_prompt, routing_context) = match connection {
            Some(connection) => {
                match services::build_acp_database_context(
                    connection,
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
    tabs: Signal<Vec<QueryTabState>>,
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

    let connection = if allow_db_read {
        active_editor_connection(tabs, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(tabs, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(tabs, active_tab_id)
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
        let (prompt, routing_context) = match connection {
            Some(connection) => {
                match services::build_acp_database_context(
                    connection,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
    allow_read_sql_run: bool,
) {
    let Some(active_sql) = active_editor_sql(tabs, active_tab_id) else {
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

    let Some(connection) = active_editor_connection(tabs, active_tab_id) else {
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
    let focus_source = active_editor_focus_source(tabs, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(tabs, active_tab_id)
    } else {
        None
    };
    let thread_history = build_thread_history_context(&panel_state().messages);

    panel_state.with_mut(|state| {
        state.busy = true;
        state.pending_sql_insert = false;
        state.suppress_transcript = false;
        state.hidden_agent_response.clear();
        state.status = "Running EXPLAIN for the active SQL...".to_string();
    });

    spawn(async move {
        let plan_output = match services::execute_query_page(
            connection.clone(),
            explain_sql.clone(),
            100,
            0,
            None,
            None,
        )
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
                connection,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
) {
    let Some(active_sql) = active_editor_sql(tabs, active_tab_id) else {
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
    let connection = if allow_db_read {
        active_editor_connection(tabs, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(tabs, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(tabs, active_tab_id)
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
        let (prompt, routing_context) = match connection {
            Some(connection) => match services::build_acp_database_context(
                connection,
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
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: u64,
    connection_label: String,
    mut chat_revision: Signal<u64>,
    allow_db_read: bool,
) {
    let Some(active_sql) = active_editor_sql(tabs, active_tab_id) else {
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
    let Some(error) = active_editor_error(tabs, active_tab_id) else {
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

    let connection = if allow_db_read {
        active_editor_connection(tabs, active_tab_id)
    } else {
        None
    };
    let focus_source = active_editor_focus_source(tabs, active_tab_id);
    let active_tab_context = if allow_db_read {
        active_editor_prompt_context(tabs, active_tab_id)
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
        let (prompt, routing_context) = match connection {
            Some(connection) => match services::build_acp_database_context(
                connection,
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
    mut tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    mut chat_revision: Signal<u64>,
    sql: String,
    execution_mode: AgentSqlExecutionMode,
    record_error_in_agent_panel: bool,
) {
    let Some(target_tab_id) = preferred_sql_target_tab_id(tabs, active_tab_id()) else {
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

    let current_tab = tabs
        .read()
        .iter()
        .find(|tab| tab.id == target_tab_id)
        .cloned();
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

    let Some(connection) = tab_connection_or_error(tabs, current_tab.id, current_tab.session_id)
    else {
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
        let resolved = match resolve_agent_sql_execution(connection.clone(), &sql).await {
            Ok(resolved) => resolved,
            Err(err) => {
                tabs.with_mut(|all_tabs| {
                    if let Some(tab) = all_tabs.iter_mut().find(|tab| tab.id == current_tab.id) {
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

        insert_sql_into_editor(panel_state, tabs, active_tab_id, resolved.sql.clone());

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
            current_tab.title,
        ));
        chat_revision += 1;

        run_query_for_tab(
            tabs,
            current_tab.id,
            connection,
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

#[cfg(test)]
mod tests {
    use super::build_explain_sql;

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
}
