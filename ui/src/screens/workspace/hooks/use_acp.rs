use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use dioxus::prelude::*;
use models::{AcpPanelState, ChatThreadSummary, QueryTabState};

use super::super::{
    actions::update_active_tab_sql,
    components::{
        AgentSqlExecutionMode,
        apply_acp_events,
        default_acp_panel_state,
        execute_agent_sql_request,
        extract_sql_candidate,
        parse_optimizer_result,
        preferred_sql_target_tab_id,
        replace_messages,
        store_optimizer_raw_response_on_tab,
        store_optimizer_result_on_tab,
    },
    helpers::{derive_chat_thread_title, reset_panel_for_thread, upsert_chat_thread_summary},
};
use crate::app_state::{
    APP_AI_FEATURES_ENABLED,
    APP_SHOW_AGENT_PANEL,
    APP_STATE,
    set_show_sql_editor,
    toast_error,
};

pub struct AcpStateInputs {
    pub chat_threads: Signal<Vec<ChatThreadSummary>>,
    pub active_chat_thread_id: Signal<Option<i64>>,
    pub chat_revision: Signal<u64>,
    pub tabs: Signal<Vec<QueryTabState>>,
    pub active_tab_id: Signal<u64>,
    pub connection_label: String,
}

#[allow(dead_code)]
pub struct AcpState {
    pub acp_panel_state: Signal<AcpPanelState>,
    pub allow_agent_db_read: Signal<bool>,
    pub allow_agent_read_sql_run: Signal<bool>,
    pub allow_agent_write_sql_run: Signal<bool>,
    pub allow_agent_tool_run: Signal<bool>,
    pub handled_agent_sql_message_id: Signal<u64>,
    pub warmed_schema_session_id: Signal<u64>,
    pub ai_disable_applied: Signal<bool>,
}

pub fn use_acp_state(inputs: AcpStateInputs) -> AcpState {
    // ── Own signals ────────────────────────────────────────────────
    let mut acp_panel_state = use_signal(default_acp_panel_state);
    let allow_agent_db_read = use_signal(|| true);
    let allow_agent_read_sql_run = use_signal(|| true);
    let allow_agent_write_sql_run = use_signal(|| false);
    let allow_agent_tool_run = use_signal(|| false);
    let mut handled_agent_sql_message_id = use_signal(|| 0_u64);
    let mut warmed_schema_session_id = use_signal(|| 0_u64);
    let mut ai_disable_applied = use_signal(|| false);

    // ── External signals (from other hooks / workspace) ────────────
    let mut chat_threads = inputs.chat_threads;
    let active_chat_thread_id = inputs.active_chat_thread_id;
    let mut chat_revision = inputs.chat_revision;
    let mut tabs = inputs.tabs;
    let mut active_tab_id = inputs.active_tab_id;
    let connection_label = inputs.connection_label;

    // Clone for the persist-chat effect closure.
    let persist_connection_label = connection_label.clone();

    // ── Effect: AI features disable ────────────────────────────────
    use_effect(move || {
        if APP_AI_FEATURES_ENABLED() {
            if ai_disable_applied() {
                ai_disable_applied.set(false);
            }
            return;
        }

        if ai_disable_applied() {
            return;
        }

        ai_disable_applied.set(true);
        let _ = services::disconnect_acp_agent();
        acp_panel_state.with_mut(|state| {
            let launch = state.launch.clone();
            let ollama = state.ollama.clone();
            let existing_messages = state.messages.clone();
            *state = AcpPanelState::new(launch, ollama);
            replace_messages(state, existing_messages);
            state.status = "AI features are disabled.".to_string();
        });
    });

    // ── Effect: load chat messages on thread switch ────────────────
    use_effect(move || {
        if !APP_AI_FEATURES_ENABLED() {
            return;
        }
        let Some(thread_id) = active_chat_thread_id() else {
            return;
        };

        spawn(async move {
            let thread_title = chat_threads
                .read()
                .iter()
                .find(|thread| thread.id == thread_id)
                .map(|thread| thread.title.clone())
                .unwrap_or_else(|| "New chat".to_string());
            let messages = match services::load_chat_thread_messages(thread_id).await {
                Ok(messages) => messages,
                Err(err) => {
                    toast_error(format!("Failed to load chat thread: {err}"));
                    Vec::new()
                }
            };
            let last_message_id = messages.iter().map(|message| message.id).max().unwrap_or(0);

            let _ = services::disconnect_acp_agent();
            handled_agent_sql_message_id.set(last_message_id);
            acp_panel_state
                .with_mut(|state| reset_panel_for_thread(state, &thread_title, messages));
        });
    });

    // ── Effect: warm schema context for ACP ────────────────────────
    use_effect(move || {
        if !APP_AI_FEATURES_ENABLED() || !allow_agent_db_read() {
            return;
        }

        let active_session = { APP_STATE.read().active_session().cloned() };
        let Some(session) = active_session else {
            return;
        };

        if warmed_schema_session_id() == session.id {
            return;
        }

        warmed_schema_session_id.set(session.id);
        spawn(async move {
            let _ = services::warm_acp_database_schema_context(
                session.connection.clone(),
                session.name.clone(),
            )
            .await;
        });
    });

    // ── Effect: persist chat on revision change ────────────────────
    use_effect(move || {
        let revision = chat_revision();
        if revision == 0 {
            return;
        }

        let connection_name = persist_connection_label.clone();
        spawn(async move {
            let Some(thread_id) = active_chat_thread_id() else {
                return;
            };

            let messages = acp_panel_state().messages.clone();
            let current_title = chat_threads
                .read()
                .iter()
                .find(|thread| thread.id == thread_id)
                .map(|thread| thread.title.clone());
            let next_title =
                derive_chat_thread_title(current_title.as_deref(), &messages, &connection_name);

            match services::save_chat_thread_snapshot(
                thread_id,
                next_title,
                connection_name.clone(),
                messages,
            )
            .await
            {
                Ok(summary) => {
                    chat_threads.with_mut(|threads| upsert_chat_thread_summary(threads, summary));
                }
                Err(err) => {
                    toast_error(format!("Failed to persist chat thread: {err}"));
                }
            }
        });
    });

    // ── Effect: ACP polling loop ───────────────────────────────────
    use_effect(move || {
        static STOP_FLAG: AtomicBool = AtomicBool::new(false);
        STOP_FLAG.store(false, Ordering::Relaxed);
        let _ = spawn(async move {
            loop {
                if STOP_FLAG.load(Ordering::Relaxed) {
                    break;
                }
                let ai_active = APP_AI_FEATURES_ENABLED();
                let panel_visible = APP_SHOW_AGENT_PANEL();
                let poll_delay = if ai_active && acp_panel_state().connected {
                    if panel_visible {
                        Duration::from_millis(120)
                    } else {
                        Duration::from_millis(180)
                    }
                } else {
                    Duration::from_millis(400)
                };

                if !ai_active {
                    let _ = services::drain_acp_events();
                    tokio::time::sleep(poll_delay).await;
                    continue;
                }

                if STOP_FLAG.load(Ordering::Relaxed) {
                    break;
                }

                let events = services::drain_acp_events();
                if !events.is_empty() {
                    acp_panel_state.with_mut(|state| apply_acp_events(state, events));
                    chat_revision += 1;

                    let pending_hidden_agent_sql = {
                        let panel_state = acp_panel_state();
                        extract_sql_candidate(&panel_state.hidden_agent_response)
                            .map(|sql| (sql, panel_state.pending_sql_insert))
                    };
                    let pending_agent_sql = if pending_hidden_agent_sql.is_none() {
                        let panel_state = acp_panel_state();
                        let handled_message_id = handled_agent_sql_message_id();
                        panel_state
                            .messages
                            .iter()
                            .filter(|message| message.id > handled_message_id)
                            .find_map(|message| match message.kind {
                                models::AcpMessageKind::Agent =>
                                    extract_sql_candidate(&message.text).map(|sql| {
                                        (message.id, sql, panel_state.pending_sql_insert)
                                    }),
                                _ => None,
                            })
                    } else {
                        None
                    };

                    if let Some((sql, pending_sql_insert)) = pending_hidden_agent_sql {
                        acp_panel_state.with_mut(|state| state.hidden_agent_response.clear());

                        if services::is_read_only_sql(&sql) && allow_agent_read_sql_run() {
                            execute_agent_sql_request(
                                acp_panel_state,
                                tabs,
                                active_tab_id,
                                chat_revision,
                                sql,
                                AgentSqlExecutionMode::AutoReadOnly,
                                false,
                            );
                        } else if pending_sql_insert
                            && let Some(target_tab_id) =
                                preferred_sql_target_tab_id(tabs, active_tab_id())
                        {
                            set_show_sql_editor(true);
                            active_tab_id.set(target_tab_id);
                            update_active_tab_sql(
                                tabs,
                                target_tab_id,
                                sql,
                                "SQL generated by ACP agent".to_string(),
                            );
                            acp_panel_state.with_mut(|state| {
                                state.busy = false;
                                state.pending_sql_insert = false;
                                state.suppress_transcript = false;
                                state.status =
                                    "Inserted generated SQL into the active editor.".to_string();
                                state.messages.retain(|message| {
                                    !matches!(message.kind, models::AcpMessageKind::Thought)
                                });
                            });
                        }
                    } else if let Some((message_id, sql, pending_sql_insert)) = pending_agent_sql {
                        handled_agent_sql_message_id.set(message_id);

                        if services::is_read_only_sql(&sql) && allow_agent_read_sql_run() {
                            execute_agent_sql_request(
                                acp_panel_state,
                                tabs,
                                active_tab_id,
                                chat_revision,
                                sql,
                                AgentSqlExecutionMode::AutoReadOnly,
                                true,
                            );
                        } else if pending_sql_insert
                            && let Some(target_tab_id) =
                                preferred_sql_target_tab_id(tabs, active_tab_id())
                        {
                            set_show_sql_editor(true);
                            active_tab_id.set(target_tab_id);
                            update_active_tab_sql(
                                tabs,
                                target_tab_id,
                                sql,
                                "SQL generated by ACP agent".to_string(),
                            );
                            acp_panel_state.with_mut(|state| {
                                state.busy = false;
                                state.pending_sql_insert = false;
                                state.suppress_transcript = false;
                                state.status =
                                    "Inserted generated SQL into the active editor.".to_string();
                                state.messages.retain(|message| {
                                    !matches!(message.kind, models::AcpMessageKind::Thought)
                                });
                            });
                        }
                    } else if !acp_panel_state().suppress_transcript
                        && !acp_panel_state().hidden_agent_response.is_empty()
                    {
                        acp_panel_state.with_mut(|state| state.hidden_agent_response.clear());
                    }

                    let optimizer_parsed = {
                        let panel_state = acp_panel_state();
                        // The buffer is only populated while `optimizer_request_active`
                        // was true, and `apply_acp_events` clears that flag on
                        // `PromptFinished`. So a non-empty buffer with the flag cleared
                        // means the full response landed in this batch. Gate on buffer
                        // non-emptiness (not the flag) so the last chunk and
                        // `PromptFinished` arriving together don't drop the result.
                        if !panel_state.optimizer_response.is_empty()
                            && !panel_state.optimizer_request_active
                        {
                            Some(parse_optimizer_result(&panel_state.optimizer_response))
                        } else {
                            None
                        }
                    };
                    if let Some(parse_result) = optimizer_parsed {
                        let target_id = active_tab_id();
                        match parse_result {
                            Ok(result) => {
                                tabs.with_mut(|all_tabs| {
                                    store_optimizer_result_on_tab(all_tabs, target_id, result);
                                });
                            }
                            Err(_) => {
                                // Invalid JSON: keep the raw text so the Analysis
                                // panel can render an "unstructured response" card.
                                let raw = {
                                    let panel_state = acp_panel_state();
                                    panel_state.optimizer_response.clone()
                                };
                                tabs.with_mut(|all_tabs| {
                                    store_optimizer_raw_response_on_tab(all_tabs, target_id, raw);
                                });
                            }
                        }
                        acp_panel_state.with_mut(|state| {
                            state.optimizer_request_active = false;
                            state.optimizer_response.clear();
                        });
                    }
                }

                tokio::time::sleep(poll_delay).await;
            }
        });
    });

    AcpState {
        acp_panel_state,
        allow_agent_db_read,
        allow_agent_read_sql_run,
        allow_agent_write_sql_run,
        allow_agent_tool_run,
        handled_agent_sql_message_id,
        warmed_schema_session_id,
        ai_disable_applied,
    }
}
