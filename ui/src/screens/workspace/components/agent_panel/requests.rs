use dioxus::prelude::*;
use models::{
    AcpMessageKind,
    AcpPanelState,
    AcpUiMessage,
    ActiveModel,
    AiCatalogSettings,
    AiProviderKind,
    AppUiSettings,
    builtin_providers,
    is_native_http_ready,
    native_http_provider_enabled,
    normalize_native_chat_url,
    provider_backend,
    provider_kind,
    provider_offers_chat,
};

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

#[derive(Clone)]
struct NativeChatParts {
    base_url: String,
    api_key: String,
    model: String,
    backend: models::AiBackendId,
    supports_thinking: bool,
    thinking_enabled: bool,
    reasoning_effort: String,
}

impl NativeChatParts {
    fn into_request(
        self,
        messages: Vec<services::NativeChatMessage>,
    ) -> services::NativeChatRequest {
        services::NativeChatRequest {
            base_url: self.base_url,
            api_key: self.api_key,
            model: self.model,
            messages,
            backend: self.backend,
            supports_thinking: self.supports_thinking,
            thinking_enabled: self.thinking_enabled,
            reasoning_effort: self.reasoning_effort,
        }
    }
}

#[derive(Clone)]
enum ActiveChatBackend {
    Native(NativeChatParts),
    Acp,
}

fn native_chat_allowed(provider: &str, catalog: &AiCatalogSettings) -> bool {
    provider_offers_chat(provider, catalog)
}

fn resolve_active_chat_backend(settings: &AppUiSettings) -> Result<ActiveChatBackend, String> {
    let Some(active) = settings.ai_catalog.active.as_ref() else {
        return Err("No language model selected.".to_string());
    };
    let api_key = settings.lm_api_key(&active.provider);
    match provider_kind(&active.provider) {
        Some(AiProviderKind::NativeHttp) => {
            if !native_chat_allowed(&active.provider, &settings.ai_catalog) {
                return Err("This provider does not support chat.".to_string());
            }
            if !native_http_provider_enabled(&settings.ai_catalog, &active.provider) {
                return Err("Enable this provider in Settings.".to_string());
            }
            if !is_native_http_ready(&active.provider, &api_key, &settings.ai_catalog) {
                return Err("Add an API key for the selected provider.".to_string());
            }
            Ok(ActiveChatBackend::Native(native_chat_parts(
                settings, active, api_key,
            )))
        }
        Some(AiProviderKind::Acp) => Ok(ActiveChatBackend::Acp),
        None => Err("Unknown language model provider.".to_string()),
    }
}

fn native_chat_parts(
    settings: &AppUiSettings,
    active: &ActiveModel,
    api_key: String,
) -> NativeChatParts {
    let provider = active.provider.as_str();
    let mut model = active.model.clone();
    if model.trim().is_empty() {
        model = vendor_model(settings, provider);
    }
    let (thinking_enabled, reasoning_effort) = native_thinking(settings, provider);
    let backend =
        provider_backend(provider, &settings.ai_catalog).expect("native http has backend");
    let supports_thinking = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
        .is_some_and(|spec| spec.supports_thinking);
    NativeChatParts {
        base_url: native_base_url(settings, provider),
        api_key,
        model,
        backend,
        supports_thinking,
        thinking_enabled,
        reasoning_effort,
    }
}

pub(super) fn native_base_url(settings: &AppUiSettings, provider: &str) -> String {
    if let Some(custom) = settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
    {
        return normalize_native_chat_url(&custom.base_url, &custom.base_url);
    }
    let default = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
        .map(|spec| spec.default_base_url)
        .unwrap_or("");
    if let Some(over) = settings.ai_catalog.overrides.get(provider)
        && !over.base_url.trim().is_empty()
    {
        return normalize_native_chat_url(&over.base_url, default);
    }
    let vendor = match provider {
        "deepseek" => settings.deepseek.base_url.as_str(),
        "openai" => settings.openai.base_url.as_str(),
        "groq" => settings.groq.base_url.as_str(),
        "openrouter" => settings.openrouter.base_url.as_str(),
        "xai" => settings.xai.base_url.as_str(),
        "mistral" => settings.mistral.base_url.as_str(),
        "ollama" => settings.ollama.base_url.as_str(),
        _ => "",
    };
    normalize_native_chat_url(vendor, default)
}

fn native_thinking(settings: &AppUiSettings, provider: &str) -> (bool, String) {
    if let Some(over) = settings.ai_catalog.overrides.get(provider) {
        return (over.thinking_enabled, over.reasoning_effort.clone());
    }
    if provider == "deepseek" {
        (
            settings.deepseek.thinking_enabled,
            settings.deepseek.reasoning_effort.clone(),
        )
    } else {
        (false, "medium".to_string())
    }
}

fn vendor_model(settings: &AppUiSettings, provider: &str) -> String {
    match provider {
        "deepseek" => settings.deepseek.model.clone(),
        "openai" => settings.openai.model.clone(),
        "groq" => settings.groq.model.clone(),
        "openrouter" => settings.openrouter.model.clone(),
        "xai" => settings.xai.model.clone(),
        "mistral" => settings.mistral.model.clone(),
        "ollama" => settings.ollama.model.clone(),
        _ => String::new(),
    }
}

fn native_history_messages(messages: &[AcpUiMessage]) -> Vec<services::NativeChatMessage> {
    messages
        .iter()
        .filter_map(|message| {
            let role = match message.kind {
                AcpMessageKind::User => "user",
                AcpMessageKind::Agent => "assistant",
                _ => return None,
            };
            let content = message.text.trim();
            if content.is_empty() {
                return None;
            }
            Some(services::NativeChatMessage {
                role: role.to_string(),
                content: content.to_string(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn submit_agent_prompt(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    backend: ActiveChatBackend,
    history: Vec<services::NativeChatMessage>,
    contextual_prompt: String,
    routing_context: String,
    on_ok: impl FnOnce(&mut AcpPanelState),
    on_err: impl FnOnce(&mut AcpPanelState, String),
) {
    match backend {
        ActiveChatBackend::Native(parts) => {
            let mut messages = history;
            messages.push(services::NativeChatMessage {
                role: "user".to_string(),
                content: contextual_prompt,
            });
            let req = parts.into_request(messages);
            panel_state.with_mut(|state| on_ok(state));
            chat_revision += 1;
            let _ = services::native_chat_prompt(req).await;
        }
        ActiveChatBackend::Acp =>
            match services::send_acp_prompt_with_routing(contextual_prompt, routing_context) {
                Ok(()) => {
                    panel_state.with_mut(|state| on_ok(state));
                    chat_revision += 1;
                }
                Err(err) => {
                    panel_state.with_mut(|state| on_err(state, err));
                    chat_revision += 1;
                }
            },
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn send_chat_prompt_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    chat_revision: Signal<u64>,
    allow_db_read: bool,
    prompt: String,
    mut prompt_draft: Signal<String>,
) {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() || panel_state().busy {
        return;
    }

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };
    let history = native_history_messages(&panel_state().messages);
    let thread_history = match &backend {
        ActiveChatBackend::Native(_) => None,
        ActiveChatBackend::Acp => build_thread_history_context(&panel_state().messages),
    };
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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            contextual_prompt,
            routing_context,
            {
                let prompt = prompt.clone();
                move |state| {
                    push_message(state, AcpMessageKind::User, prompt);
                    state.prompt.clear();
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for agent response...".to_string();
                }
            },
            |state, err| {
                state.status = err.clone();
                state.busy = false;
                push_message(state, AcpMessageKind::Error, err);
            },
        )
        .await;
        prompt_draft.set(String::new());
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
    chat_revision: Signal<u64>,
    allow_db_read: bool,
    qualified_name: String,
) {
    if panel_state().busy {
        return;
    }

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };
    let history = native_history_messages(&panel_state().messages);

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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            contextual_prompt,
            routing_context,
            move |state| {
                push_message(
                    state,
                    AcpMessageKind::User,
                    format!("Describe: {qualified_name}"),
                );
                state.busy = true;
                state.pending_sql_insert = false;
                state.status = "Waiting for agent response...".to_string();
            },
            |state, err| {
                state.status = err.clone();
                state.busy = false;
                push_message(state, AcpMessageKind::Error, err);
            },
        )
        .await;
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn send_sql_generation_request(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    active_tab_id: u64,
    connection_label: String,
    chat_revision: Signal<u64>,
    allow_db_read: bool,
    prompt: String,
    mut prompt_draft: Option<Signal<String>>,
    record_in_agent_panel: bool,
) {
    let request = prompt.trim().to_string();
    if request.is_empty() || panel_state().busy {
        return;
    }

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };

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
    let history = native_history_messages(&panel_state().messages);
    let thread_history = match &backend {
        ActiveChatBackend::Native(_) => None,
        ActiveChatBackend::Acp => build_thread_history_context(&panel_state().messages),
    };
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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            prompt,
            routing_context,
            move |state| {
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
            },
            move |state, err| {
                state.status = err.clone();
                state.busy = false;
                state.pending_sql_insert = false;
                state.suppress_transcript = false;
                state.hidden_agent_response.clear();
                if record_in_agent_panel {
                    push_message(state, AcpMessageKind::Error, err);
                }
            },
        )
        .await;
        if let Some(prompt_draft) = prompt_draft.as_mut() {
            prompt_draft.set(String::new());
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

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };

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
    let history = native_history_messages(&panel_state().messages);
    let thread_history = match &backend {
        ActiveChatBackend::Native(_) => None,
        ActiveChatBackend::Acp => build_thread_history_context(&panel_state().messages),
    };

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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            prompt,
            routing_context,
            {
                let active_sql = active_sql.clone();
                move |state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Explain query plan:\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for query plan explanation...".to_string();
                }
            },
            |state, err| {
                state.status = err.clone();
                state.busy = false;
                push_message(state, AcpMessageKind::Error, err);
            },
        )
        .await;
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

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };
    let history = native_history_messages(&panel_state().messages);
    let thread_history = match &backend {
        ActiveChatBackend::Native(_) => None,
        ActiveChatBackend::Acp => build_thread_history_context(&panel_state().messages),
    };
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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            prompt,
            routing_context,
            {
                let active_sql = active_sql.clone();
                move |state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Explain active SQL:\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = false;
                    state.status = "Waiting for SQL explanation...".to_string();
                }
            },
            |state, err| {
                state.status = err.clone();
                state.busy = false;
                push_message(state, AcpMessageKind::Error, err);
            },
        )
        .await;
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

    let settings = crate::app_state::APP_UI_SETTINGS();
    let backend = match resolve_active_chat_backend(&settings) {
        Ok(backend) => backend,
        Err(err) => {
            crate::app_state::toast_error(err);
            return;
        }
    };

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
    let history = native_history_messages(&panel_state().messages);
    let thread_history = match &backend {
        ActiveChatBackend::Native(_) => None,
        ActiveChatBackend::Acp => build_thread_history_context(&panel_state().messages),
    };

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

        submit_agent_prompt(
            panel_state,
            chat_revision,
            backend,
            history,
            prompt,
            routing_context,
            {
                let error = error.clone();
                let active_sql = active_sql.clone();
                move |state| {
                    push_message(
                        state,
                        AcpMessageKind::User,
                        format!("Fix SQL error: {error}\n```sql\n{active_sql}\n```"),
                    );
                    state.busy = true;
                    state.pending_sql_insert = true;
                    state.status = "Waiting for repaired SQL...".to_string();
                }
            },
            |state, err| {
                state.status = err.clone();
                state.busy = false;
                state.pending_sql_insert = false;
                push_message(state, AcpMessageKind::Error, err);
            },
        )
        .await;
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
    use super::{
        AgentSqlExecutionMode,
        build_explain_sql,
        native_base_url,
        native_chat_allowed,
        read_only_agent_sql_blocked,
        resolve_active_chat_backend,
    };
    use models::{
        ActiveModel,
        AiBackendId,
        AiCatalogSettings,
        AiProviderOverride,
        AppUiSettings,
        CustomNativeProvider,
    };

    #[test]
    fn native_base_url_prefers_custom_then_override() {
        let mut settings = AppUiSettings::default();
        settings
            .ai_catalog
            .custom_native
            .push(CustomNativeProvider {
                id: "custom:1".into(),
                name: "Mine".into(),
                base_url: "http://localhost:8080/".into(),
                models: Vec::new(),
                backend: AiBackendId::OpenAiCompat,
            });
        assert_eq!(
            native_base_url(&settings, "custom:1"),
            "http://localhost:8080"
        );
        settings.ai_catalog.overrides.insert(
            "openai".into(),
            AiProviderOverride {
                base_url: "https://example.com/v1/".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            native_base_url(&settings, "openai"),
            "https://example.com/v1"
        );
    }

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

    #[test]
    fn native_chat_allowed_skips_complete_only_codestral() {
        let catalog = AiCatalogSettings::default();
        assert!(!native_chat_allowed("codestral", &catalog));
        assert!(native_chat_allowed("openai", &catalog));
    }

    #[test]
    fn resolve_active_chat_backend_rejects_complete_only_codestral() {
        let mut settings = AppUiSettings::default();
        settings.ai_catalog.active = Some(ActiveModel {
            provider: "codestral".into(),
            model: "codestral-latest".into(),
        });
        settings.ai_catalog.overrides.insert(
            "codestral".into(),
            AiProviderOverride {
                enabled: true,
                ..Default::default()
            },
        );
        settings.set_lm_api_key("codestral", "sk-test".into());
        match resolve_active_chat_backend(&settings) {
            Err(err) => assert!(
                err.to_ascii_lowercase().contains("chat"),
                "expected a chat-capability error, got {err}"
            ),
            Ok(_) => panic!("complete-only Codestral must not chat"),
        }
    }

    #[test]
    fn resolve_active_chat_backend_accepts_enabled_openai() {
        let mut settings = AppUiSettings::default();
        settings.ai_catalog.active = Some(ActiveModel {
            provider: "openai".into(),
            model: "gpt-5.6-sol".into(),
        });
        settings.ai_catalog.overrides.insert(
            "openai".into(),
            AiProviderOverride {
                enabled: true,
                ..Default::default()
            },
        );
        settings.set_lm_api_key("openai", "sk-test".into());
        assert!(resolve_active_chat_backend(&settings).is_ok());
    }
}
