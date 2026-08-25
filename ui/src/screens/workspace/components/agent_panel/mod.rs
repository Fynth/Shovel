mod clickhouse;
mod composer;
mod messages;
pub(crate) mod prompt;
mod registry_card;
mod requests;
mod setup;
mod state;

use dioxus::prelude::*;
use models::{
    AcpMessageKind,
    AcpPanelState,
    ChatArtifact,
    ChatThreadSummary,
    WorkspaceToolPanel,
};

use crate::{
    app_state::{
        APP_UI_SETTINGS,
        is_panel_collapsed,
        set_deepseek_api_key,
        set_deepseek_base_url,
        set_deepseek_enabled,
        set_deepseek_model,
        set_deepseek_reasoning_effort,
        set_deepseek_thinking_enabled,
        set_ollama_api_key,
        set_ollama_base_url,
        set_ollama_enabled,
        set_ollama_model,
        toggle_panel_collapsed,
    },
    screens::workspace::tab_store::TabStore,
};

use super::{ActionIcon, Chevron, IconButton};

use self::{
    composer::AgentComposer,
    messages::{
        AGENT_MESSAGE_BATCH,
        MessageChunk,
        acp_registry_loading_text,
        acp_registry_preparing_text,
        artifact_title,
        build_thread_meta,
        code_chunk_sql,
        compact_header_title,
        copy_text_to_clipboard,
        has_in_progress_agent_message,
        is_visible_message,
        last_chunk_is_text,
        parse_message_chunks,
        render_message_markdown_html,
        rich_busy_label,
        should_render_message_text,
    },
    registry_card::RegistryAgentCard,
    requests::can_execute_agent_sql,
    setup::{
        AgentSetupMode,
        connect_embedded_deepseek,
        connect_embedded_ollama,
        setup_mode_button_class,
    },
    state::{
        message_kind_avatar,
        message_kind_class,
        message_kind_label,
        permission_button_class,
        push_message,
    },
};

pub(crate) use self::{
    prompt::{extract_sql_candidate, insert_sql_into_editor, preferred_sql_target_tab_id},
    requests::{
        execute_agent_sql_request,
        send_describe_object_request,
        send_sql_explanation_request,
        send_sql_generation_request,
        send_sql_plan_request,
    },
    setup::ensure_default_sql_agent_connected,
    state::{apply_acp_events, default_acp_panel_state, replace_messages},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentSqlExecutionMode {
    Manual,
    AutoReadOnly,
}

#[component]
pub fn AcpAgentPanel(
    mut panel_state: Signal<AcpPanelState>,
    store: TabStore,
    mut chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    chat_threads: Vec<ChatThreadSummary>,
    active_thread_id: Option<i64>,
    thread_title: String,
    thread_connection_name: String,
    sql_connection_label: String,
    on_new_thread: EventHandler<()>,
    on_select_thread: EventHandler<i64>,
    on_delete_thread: EventHandler<i64>,
) -> Element {
    let state = panel_state();
    let thread_title = compact_header_title(&thread_title);
    let thread_meta = build_thread_meta(&thread_connection_name, &state);
    let chat_label = sql_connection_label.clone();
    let collapsed = is_panel_collapsed(WorkspaceToolPanel::Agent);
    let mut setup_mode = use_signal(|| AgentSetupMode::DeepSeek);
    let mut show_dialogs = use_signal(|| false);
    let mut registry_busy = use_signal(|| false);
    let mut registry_status = use_signal(String::new);
    let registry_agents =
        use_resource(move || async move { services::load_acp_registry_agents().await });
    let registry_result = registry_agents();
    let selected_registry_mode = setup_mode();
    let selected_registry_agent = selected_registry_mode
        .registry_agent_id()
        .and_then(|agent_id| {
            registry_result
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .and_then(|agents| agents.iter().find(|agent| agent.id == agent_id))
                .cloned()
        });
    let deepseek_settings = use_memo(move || APP_UI_SETTINGS().deepseek);
    let deepseek_settings = deepseek_settings();
    let ollama_settings = use_memo(move || APP_UI_SETTINGS().ollama);
    let ollama_settings = ollama_settings();
    let visible_messages = state
        .messages
        .clone()
        .into_iter()
        .filter(is_visible_message)
        .collect::<Vec<_>>();
    let visible_message_total = visible_messages.len();
    let mut message_limit = use_signal(|| AGENT_MESSAGE_BATCH);
    let hidden_message_count = visible_message_total.saturating_sub(message_limit());
    let rendered_messages = visible_messages
        .iter()
        .skip(hidden_message_count)
        .cloned()
        .collect::<Vec<_>>();
    let busy_label = rich_busy_label(&state);
    let streaming_active = has_in_progress_agent_message(&state);
    use_effect(move || {
        let _ = active_thread_id;
        message_limit.set(AGENT_MESSAGE_BATCH);
    });

    // Keep the transcript pinned to the newest message while it streams
    // in, but only when the user is already near the bottom so reading
    // older history is not interrupted.
    use_effect(move || {
        let _ = chat_revision();
        let _ = visible_message_total;
        let _ = streaming_active;
        let _ = document::eval(
            r#"
            (() => {
                const el = document.getElementById("agent-panel-messages");
                if (!el) return;
                const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
                if (nearBottom) {
                    el.scrollTop = el.scrollHeight;
                }
            })()
            "#,
        );
    });

    use_effect(move || {
        let Some(permission_request) = panel_state().pending_permission.clone() else {
            return;
        };

        if allow_agent_tool_run() {
            return;
        }

        match services::respond_acp_permission(permission_request.request_id, None) {
            Ok(()) => {
                panel_state.with_mut(|state| {
                    state.pending_permission = None;
                    state.status = "Blocked ACP tool request because tools/code execution is disabled.".to_string();
                    push_message(
                        state,
                        AcpMessageKind::System,
                        format!(
                            "Blocked ACP tool request because tools/code execution is disabled.\n{}",
                            permission_request.tool_summary
                        ),
                    );
                });
                chat_revision += 1;
            }
            Err(err) => {
                panel_state.with_mut(|state| {
                    state.status = err.clone();
                    push_message(state, AcpMessageKind::Error, err);
                });
                chat_revision += 1;
            }
        }
    });

    rsx! {
        aside { class: "agent-panel",
            div { class: "agent-panel__header",
                div { class: "agent-panel__header-copy",
                    h3 { class: "agent-panel__title", {thread_title} }
                    if !thread_meta.is_empty() {
                        p { class: "agent-panel__meta", {thread_meta} }
                    }
                }
                div { class: "agent-panel__header-actions",
                    button {
                        class: "workspace__panel-collapse",
                        "aria-label": if collapsed {
                            "Expand agent panel"
                        } else {
                            "Collapse agent panel"
                        },
                        "aria-expanded": "{!collapsed}",
                        onclick: move |_| toggle_panel_collapsed(WorkspaceToolPanel::Agent),
                        Chevron { open: !collapsed }
                    }
                    button {
                        class: if show_dialogs() {
                            "button button--ghost button--small button--active"
                        } else {
                            "button button--ghost button--small"
                        },
                        onclick: move |_| show_dialogs.set(!show_dialogs()),
                        "Dialogs"
                    }
                    if state.connected && state.busy {
                        IconButton {
                            icon: ActionIcon::Clear,
                            label: "Cancel request".to_string(),
                            onclick: move |_| {
                                if let Err(err) = services::cancel_acp_prompt() {
                                    panel_state.with_mut(|state| {
                                        state.status = err.clone();
                                        push_message(state, AcpMessageKind::Error, err);
                                    });
                                    chat_revision += 1;
                                } else {
                                    panel_state.with_mut(|state| {
                                        state.status = "Cancelling prompt...".to_string();
                                    });
                                }
                            },
                            small: true,
                            disabled: !state.busy,
                        }
                    }
                    if state.connected {
                        IconButton {
                            icon: ActionIcon::Close,
                            label: "Disconnect agent".to_string(),
                            onclick: move |_| {
                                let _ = services::disconnect_acp_agent();
                                panel_state.with_mut(|state| {
                                    state.connected = false;
                                    state.busy = false;
                                    state.pending_sql_insert = false;
                                    state.connection = None;
                                    state.status = "ACP agent is disconnected.".to_string();
                                });
                            },
                            small: true,
                        }
                    }
                }
            }
            if show_dialogs() {
                div {
                    class: "agent-panel__dialogs-popover",
                    onclick: move |event| event.stop_propagation(),
                    div { class: "agent-panel__dialogs-header",
                        div { class: "agent-panel__dialogs-copy",
                            h4 { class: "agent-panel__section-title", "Dialogs" }
                            p { class: "agent-panel__hint", "Switch or create a persistent database chat." }
                        }
                        button {
                            class: "button button--primary button--small",
                            onclick: move |_| {
                                on_new_thread.call(());
                                show_dialogs.set(false);
                            },
                            "New chat"
                        }
                    }
                    div { class: "agent-panel__dialogs-list",
                        if chat_threads.is_empty() {
                            p { class: "empty-state", "No saved dialogs yet." }
                        } else {
                            for thread in chat_threads {
                                article {
                                    class: if Some(thread.id) == active_thread_id {
                                        "agent-panel__dialog-item agent-panel__dialog-item--active"
                                    } else {
                                        "agent-panel__dialog-item"
                                    },
                                    button {
                                        class: "agent-panel__dialog-main",
                                        onclick: {
                                            let thread_id = thread.id;
                                            move |_| {
                                                on_select_thread.call(thread_id);
                                                show_dialogs.set(false);
                                            }
                                        },
                                        div { class: "agent-panel__dialog-copy",
                                            p { class: "agent-panel__dialog-title", "{thread.title}" }
                                            p { class: "agent-panel__dialog-meta", "{thread.connection_name}" }
                                            if !thread.last_message_preview.trim().is_empty() {
                                                p {
                                                    class: "agent-panel__dialog-preview",
                                                    "{thread.last_message_preview}"
                                                }
                                            }
                                        }
                                    }
                                    button {
                                        class: "agent-panel__dialog-delete",
                                        title: "Delete dialog",
                                        onclick: {
                                            let thread_id = thread.id;
                                            move |event| {
                                                event.stop_propagation();
                                                on_delete_thread.call(thread_id);
                                            }
                                        },
                                        "x"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !collapsed {
            if state.connected {
                div { class: "agent-panel__session",
                    div {
                        id: "agent-panel-messages",
                        class: "agent-panel__messages",
                        if visible_messages.is_empty() {
                            p { class: "empty-state", "Ask for SQL, schema, or data help." }
                        } else {
                            if hidden_message_count > 0 {
                                div { class: "agent-panel__message-actions",
                                    button {
                                        class: "button button--ghost button--small",
                                        onclick: move |_| {
                                            message_limit.set(
                                                (message_limit() + AGENT_MESSAGE_BATCH)
                                                    .min(visible_message_total),
                                            );
                                        },
                                        "Show {hidden_message_count.min(AGENT_MESSAGE_BATCH)} older messages"
                                    }
                                    button {
                                        class: "button button--ghost button--small",
                                        onclick: move |_| message_limit.set(visible_message_total),
                                        "Show all"
                                    }
                                }
                            }
                            for (message_index, message) in rendered_messages.iter().cloned().enumerate() {
                                {
                                    let is_last_visible_message =
                                        message_index + 1 == rendered_messages.len();
                                    let show_streaming_caret = streaming_active
                                        && is_last_visible_message
                                        && matches!(message.kind, AcpMessageKind::Agent);
                                    let is_in_progress_message = matches!(
                                        message.kind,
                                        AcpMessageKind::Agent | AcpMessageKind::Thought
                                    ) && state.busy
                                        && is_last_visible_message;
                                    let message_chunks = parse_message_chunks(&message.text);
                                    let has_sql_chunk =
                                        message_chunks.iter().any(|chunk| match chunk {
                                            MessageChunk::Code { language, code } => {
                                                code_chunk_sql(language.as_deref(), code).is_some()
                                            }
                                            MessageChunk::Text(_) => false,
                                        });
                                    let thinking_label = if is_in_progress_message {
                                        busy_label
                                    } else {
                                        ""
                                    };
                                    let message_class = if is_in_progress_message {
                                        format!(
                                            "agent-panel__message agent-panel__message--{} agent-panel__message--in-progress",
                                            message_kind_class(&message.kind),
                                        )
                                    } else {
                                        format!(
                                            "agent-panel__message agent-panel__message--{}",
                                            message_kind_class(&message.kind),
                                        )
                                    };
                                    let append_streaming_caret =
                                        show_streaming_caret && last_chunk_is_text(&message_chunks);

                                    rsx! {
                                        article {
                                            class: message_class,
                                            div { class: "agent-panel__message-meta",
                                                span { class: "agent-panel__message-avatar",
                                                    "{message_kind_avatar(&message.kind)}"
                                                }
                                                p { class: "agent-panel__message-role", "{message_kind_label(&message.kind)}" }
                                                if matches!(message.kind, AcpMessageKind::Thought) {
                                                    div { class: "agent-panel__thinking",
                                                        span { class: "agent-panel__thinking-orb" }
                                                        span { class: "agent-panel__thinking-dots",
                                                            span { class: "agent-panel__thinking-dot" }
                                                            span { class: "agent-panel__thinking-dot" }
                                                            span { class: "agent-panel__thinking-dot" }
                                                        }
                                                        span {
                                                            class: "agent-panel__thinking-label",
                                                            {thinking_label.to_string()}
                                                        }
                                                    }
                                                }
                                            }
                                            if should_render_message_text(&message) {
                                                for chunk in message_chunks {
                                                    match chunk {
                                                        MessageChunk::Text(text) => {
                                                            let rendered_html = render_message_markdown_html(&text);
                                                            rsx! {
                                                                div {
                                                                    class: "agent-panel__message-text agent-panel__message-markdown",
                                                                    dangerous_inner_html: rendered_html,
                                                                }
                                                            }
                                                        },
                                                        MessageChunk::Code { language, code } => {
                                                            let sql = code_chunk_sql(language.as_deref(), &code);
                                                            let language_label = language
                                                                .clone()
                                                                .filter(|value| !value.trim().is_empty())
                                                                .unwrap_or_else(|| {
                                                                    if sql.is_some() {
                                                                        "SQL".to_string()
                                                                    } else {
                                                                        "Code".to_string()
                                                                    }
                                                                });

                                                            rsx! {
                                                                div { class: "agent-panel__code-card",
                                                                    div { class: "agent-panel__code-header",
                                                                        span { class: "agent-panel__code-language", {language_label.to_string()} }
                                                                    div { class: "agent-panel__code-actions",
                                                                        button {
                                                                            class: "button button--ghost button--small",
                                                                            onclick: {
                                                                                let copy_value = sql.clone().unwrap_or_else(|| code.clone());
                                                                                let copy_label = if sql.is_some() { "SQL" } else { "code" };
                                                                                move |event| {
                                                                                    event.stop_propagation();
                                                                                    copy_text_to_clipboard(panel_state, copy_value.clone(), copy_label);
                                                                                }
                                                                            },
                                                                            if sql.is_some() { "Copy SQL" } else { "Copy" }
                                                                        }
                                                                        if let Some(sql) = sql.clone() {
                                                                            SqlActionButtons {
                                                                                panel_state,
                                                                                store,
                                                                                chat_revision,
                                                                                allow_agent_read_sql_run,
                                                                                allow_agent_write_sql_run,
                                                                                sql,
                                                                            }
                                                                        }
                                                                    }
                                                                    }
                                                                    pre { class: "agent-panel__code-body", {code.to_string()} }
                                                                }
                                                             }
                                                         }
                                                     }
                                                 }
                                             }
                                            if append_streaming_caret {
                                                span {
                                                    class: "agent-panel__streaming-caret",
                                                    aria_hidden: "true",
                                                }
                                            }
                                            if let Some(artifact) = message.artifact.clone() {
                                                match artifact {
                                                    ChatArtifact::SqlDraft { sql } => rsx! {
                                                        div { class: "agent-panel__artifact",
                                                            div { class: "agent-panel__artifact-header",
                                                                p { class: "agent-panel__artifact-label", {artifact_title(&ChatArtifact::SqlDraft { sql: sql.clone() })} }
                                                                div { class: "agent-panel__artifact-actions",
                                                                    SqlActionButtons {
                                                                        panel_state,
                                                                        store,
                                                                        chat_revision,
                                                                        allow_agent_read_sql_run,
                                                                        allow_agent_write_sql_run,
                                                                        sql: sql.clone(),
                                                                    }
                                                                }
                                                            }
                                                            pre { class: "agent-panel__artifact-body", {sql.to_string()} }
                                                        }
                                                    },
                                                    ChatArtifact::QuerySummary { sql, summary: _ } => rsx! {
                                                        div { class: "agent-panel__artifact",
                                                            div { class: "agent-panel__artifact-header",
                                                                p { class: "agent-panel__artifact-label", {artifact_title(&ChatArtifact::QuerySummary { sql: sql.clone(), summary: String::new() })} }
                                                                div { class: "agent-panel__artifact-actions",
                                                                    SqlActionButtons {
                                                                        panel_state,
                                                                        store,
                                                                        chat_revision,
                                                                        allow_agent_read_sql_run,
                                                                        allow_agent_write_sql_run,
                                                                        sql: sql.clone(),
                                                                    }
                                                                }
                                                            }
                                                            pre { class: "agent-panel__artifact-body", {sql.to_string()} }
                                                        }
                                                    },
                                                }
                                            }
                                            if matches!(message.kind, AcpMessageKind::Agent) && !has_sql_chunk {
                                                if let Some(sql) = extract_sql_candidate(&message.text) {
                                                    {
                                                        let sql_is_read_only = services::is_read_only_sql(&sql);
                                                        rsx! {
                                                            div { class: "agent-panel__message-actions",
                                                                SqlActionButtons {
                                                                    panel_state,
                                                                    store,
                                                                    chat_revision,
                                                                    allow_agent_read_sql_run,
                                                                    allow_agent_write_sql_run,
                                                                    sql,
                                                                    run_label: { if sql_is_read_only { "Run again" } else { "Run SQL" } },
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(permission_request) = state.pending_permission.clone() {
                        div { class: "agent-panel__permission",
                            div {
                                class: "agent-panel__permission-copy",
                                p { class: "agent-panel__message-role", "Permission" }
                                pre { class: "agent-panel__message-body", "{permission_request.tool_summary}" }
                            }
                            div { class: "agent-panel__permission-actions",
                                for option in permission_request.options {
                                    button {
                                        class: permission_button_class(&option.kind),
                                        onclick: {
                                            let request_id = permission_request.request_id;
                                            let option_id = option.option_id.clone();
                                            let label = option.label.clone();
                                            move |_| {
                                                match services::respond_acp_permission(
                                                    request_id,
                                                    Some(option_id.clone()),
                                                ) {
                                                    Ok(()) => {
                                                        panel_state.with_mut(|state| {
                                                            state.pending_permission = None;
                                                            state.status = format!("Permission response sent: {label}");
                                                            push_message(
                                                                state,
                                                                AcpMessageKind::System,
                                                                format!("Selected permission option: {label}"),
                                                            );
                                                        });
                                                        chat_revision += 1;
                                                    }
                                                    Err(err) => {
                                                        panel_state.with_mut(|state| {
                                                            state.status = err.clone();
                                                            push_message(state, AcpMessageKind::Error, err);
                                                        });
                                                        chat_revision += 1;
                                                    }
                                                }
                                            }
                                        },
                                        "{option.label}"
                                    }
                                }
                                button {
                                    class: "button button--ghost button--small",
                                    onclick: {
                                        let request_id = permission_request.request_id;
                                        move |_| {
                                            match services::respond_acp_permission(request_id, None) {
                                                Ok(()) => {
                                                    panel_state.with_mut(|state| {
                                                        state.pending_permission = None;
                                                        state.status = "Permission cancelled.".to_string();
                                                        push_message(
                                                            state,
                                                            AcpMessageKind::System,
                                                            "Cancelled permission request.".to_string(),
                                                        );
                                                    });
                                                    chat_revision += 1;
                                                }
                                                Err(err) => {
                                                    panel_state.with_mut(|state| {
                                                        state.status = err.clone();
                                                        push_message(state, AcpMessageKind::Error, err);
                                                    });
                                                    chat_revision += 1;
                                                }
                                            }
                                        }
                                    },
                                },
                                "Cancel"
                            }
                        }
                    }

                    AgentComposer {
                        key: format!("{:?}-{}", active_thread_id, state.connected),
                        panel_state,
                        store,
                        chat_revision,
                        allow_agent_db_read,
                        allow_agent_read_sql_run,
                        allow_agent_write_sql_run,
                        allow_agent_tool_run,
                        busy: state.busy,
                        connection_label: chat_label.clone(),
                        reset_key: format!("{:?}-{}", active_thread_id, state.connected),
                    }
                }
            } else {
                div { class: "agent-panel__connect",
                    div { class: "agent-panel__mode-switch",
                        for mode in AgentSetupMode::ALL {
                            button {
                                class: setup_mode_button_class(mode, setup_mode()),
                                onclick: move |_| setup_mode.set(mode),
                                span { class: "agent-panel__mode-name", "{mode.label()}" }
                                span { class: "agent-panel__mode-kind", "{mode.meta()}" }
                            }
                        }
                    }

                    {match setup_mode() {
                        AgentSetupMode::DeepSeek => rsx! {
                            div { class: "agent-panel__section",
                                div { class: "agent-panel__section-header",
                                    div { class: "agent-panel__section-copy",
                                        h4 { class: "agent-panel__section-title", "Built-in DeepSeek ACP" }
                                        p { class: "agent-panel__hint", "Cloud model via DeepSeek API key. Uses Shovel database context and SQL workflows." }
                                    }
                                    span { class: "agent-panel__badge", "API key" }
                                }
                                label {
                                    class: "settings-modal__toggle",
                                    input {
                                        r#type: "checkbox",
                                        checked: deepseek_settings.enabled,
                                        disabled: deepseek_settings.api_key.trim().is_empty(),
                                        oninput: move |event| {
                                            set_deepseek_enabled(event.checked());
                                        },
                                    }
                                    span { "Use DeepSeek for automatic SQL agent actions" }
                                }
                                div { class: "agent-panel__field-grid",
                                    div { class: "field",
                                        label { class: "field__label", "Base URL" }
                                        input {
                                            class: "input",
                                            value: "{deepseek_settings.base_url}",
                                            placeholder: "https://api.deepseek.com",
                                            oninput: move |event| {
                                                set_deepseek_base_url(event.value());
                                            }
                                        }
                                    }
                                    div { class: "field",
                                        label { class: "field__label", "Model" }
                                        input {
                                            class: "input",
                                            value: "{deepseek_settings.model}",
                                            placeholder: "deepseek-v4-pro",
                                            oninput: move |event| {
                                                set_deepseek_model(event.value());
                                            }
                                        }
                                    }
                                }
                                div { class: "field",
                                    label { class: "field__label", "API key" }
                                    input {
                                        class: "input",
                                        r#type: "password",
                                        value: "{deepseek_settings.api_key}",
                                        placeholder: "sk-...",
                                        oninput: move |event| {
                                            set_deepseek_api_key(event.value());
                                        }
                                    }
                                }
                                div { class: "agent-panel__field-grid",
                                    div { class: "field",
                                        label { class: "field__label", "Reasoning effort" }
                                        select {
                                            class: "input",
                                            value: "{deepseek_settings.reasoning_effort}",
                                            oninput: move |event| {
                                                set_deepseek_reasoning_effort(event.value());
                                            },
                                            option { value: "low", "low" }
                                            option { value: "medium", "medium" }
                                            option { value: "high", "high" }
                                        }
                                    }
                                    label {
                                        class: "settings-modal__toggle",
                                        input {
                                            r#type: "checkbox",
                                            checked: deepseek_settings.thinking_enabled,
                                            oninput: move |event| {
                                                set_deepseek_thinking_enabled(event.checked());
                                            },
                                        }
                                        span { "Thinking mode" }
                                    }
                                }
                                button {
                                    class: "button button--primary button--small",
                                    disabled: state.busy
                                        || deepseek_settings.api_key.trim().is_empty()
                                        || deepseek_settings.model.trim().is_empty(),
                                    onclick: move |_| {
                                        let deepseek = APP_UI_SETTINGS().deepseek;
                                        spawn(async move {
                                            if let Err(err) = connect_embedded_deepseek(
                                                panel_state,
                                                chat_revision,
                                                deepseek,
                                            ).await {
                                                panel_state.with_mut(|state| {
                                                    state.status = err;
                                                });
                                            }
                                        });
                                    },
                                    "Connect DeepSeek"
                                }
                                if deepseek_settings.api_key.trim().is_empty() {
                                    p {
                                        class: "agent-panel__hint",
                                        "Add a DeepSeek API key here or in Settings to enable this bridge."
                                    }
                                }
                            }
                        },
                        AgentSetupMode::Ollama => rsx! {
                            div { class: "agent-panel__section",
                                div { class: "agent-panel__section-header",
                                    div { class: "agent-panel__section-copy",
                                        h4 { class: "agent-panel__section-title", "Ollama" }
                                        p { class: "agent-panel__hint", "Local or remote `/api` endpoint." }
                                    }
                                    span { class: "agent-panel__badge", "Embedded" }
                                }
                                label {
                                    class: "settings-modal__toggle",
                                    input {
                                        r#type: "checkbox",
                                        checked: ollama_settings.enabled,
                                        disabled: ollama_settings.model.trim().is_empty(),
                                        oninput: move |event| {
                                            set_ollama_enabled(event.checked());
                                        },
                                    }
                                    span { "Auto-connect Ollama on launch" }
                                }
                                div { class: "agent-panel__field-grid",
                                    div { class: "field",
                                        label { class: "field__label", "Base URL" }
                                        input {
                                            class: "input",
                                            value: "{ollama_settings.base_url}",
                                            placeholder: "http://localhost:11434/api",
                                            oninput: move |event| {
                                                set_ollama_base_url(event.value());
                                            }
                                        }
                                    }
                                    div { class: "field",
                                        label { class: "field__label", "Model" }
                                        input {
                                            class: "input",
                                            value: "{ollama_settings.model}",
                                            placeholder: "qwen3:latest",
                                            oninput: move |event| {
                                                set_ollama_model(event.value());
                                            }
                                        }
                                    }
                                }
                                div { class: "field",
                                    label { class: "field__label", "API key" }
                                    input {
                                        class: "input",
                                        r#type: "password",
                                        value: "{ollama_settings.api_key}",
                                        placeholder: "Optional bearer token",
                                        oninput: move |event| {
                                            set_ollama_api_key(event.value());
                                        }
                                    }
                                }
                                button {
                                    class: "button button--primary button--small",
                                    disabled: state.busy || ollama_settings.model.trim().is_empty(),
                                    onclick: move |_| {
                                        let ollama = APP_UI_SETTINGS().ollama;
                                        spawn(async move {
                                            if let Err(err) = connect_embedded_ollama(
                                                panel_state,
                                                chat_revision,
                                                ollama,
                                            ).await {
                                                panel_state.with_mut(|state| {
                                                    state.status = err;
                                                });
                                            }
                                        });
                                    },
                                    "Connect Ollama"
                                }
                            }
                        },
                        AgentSetupMode::OpenCode | AgentSetupMode::Codex => rsx! {
                            div { class: "agent-panel__section",
                                {
                                    let registry_name = selected_registry_mode
                                        .registry_name()
                                        .unwrap_or("Registry agent");
                                    let registry_hint = selected_registry_mode
                                        .registry_hint()
                                        .unwrap_or("Quick start an agent.");
                                    let registry_agent_id = selected_registry_mode
                                        .registry_agent_id()
                                        .unwrap_or_default()
                                        .to_string();

                                    rsx! {
                                div { class: "agent-panel__section-header",
                                    div { class: "agent-panel__section-copy",
                                        h4 { class: "agent-panel__section-title", {registry_name.to_string()} }
                                        p { class: "agent-panel__hint", {registry_hint.to_string()} }
                                    }
                                    span { class: "agent-panel__badge", "Registry" }
                                }
                                if !registry_status().trim().is_empty() {
                                    p { class: "agent-panel__hint agent-panel__hint--status", "{registry_status}" }
                                }
                                if let Some(agent) = selected_registry_agent {
                                    RegistryAgentCard {
                                        agent,
                                        busy: registry_busy(),
                                        on_connect: move |_| {
                                            let cwd = panel_state().launch.cwd.clone();
                                            let registry_name = registry_name.to_string();
                                            let registry_agent_id = registry_agent_id.clone();
                                            registry_busy.set(true);
                                            registry_status.set(acp_registry_preparing_text(&registry_name));
                                            spawn(async move {
                                                match services::install_acp_registry_agent(registry_agent_id, cwd).await {
                                                    Ok(launch) => {
                                                        panel_state.with_mut(|state| {
                                                            state.launch = launch.clone();
                                                            state.busy = true;
                                                            state.status =
                                                                format!("Connecting to {registry_name}...");
                                                        });
                                                        match services::connect_acp_agent(launch).await {
                                                            Ok(connection) => {
                                                                panel_state.with_mut(|state| {
                                                                    state::apply_connected(state, connection);
                                                                });
                                                                registry_status.set(format!(
                                                                    "{registry_name} connected."
                                                                ));
                                                            }
                                                            Err(err) => {
                                                                panel_state.with_mut(|state| {
                                                                    state.busy = false;
                                                                    state.connected = false;
                                                                    state.connection = None;
                                                                    state.status = err.clone();
                                                                    push_message(state, AcpMessageKind::Error, err.clone());
                                                                });
                                                                chat_revision += 1;
                                                                registry_status.set(err);
                                                            }
                                                        }
                                                    }
                                                    Err(err) => {
                                                        panel_state.with_mut(|state| {
                                                            state.status = err.clone();
                                                            push_message(state, AcpMessageKind::Error, err.clone());
                                                        });
                                                        chat_revision += 1;
                                                        registry_status.set(err);
                                                    }
                                                }
                                                registry_busy.set(false);
                                            });
                                        }
                                    }
                                } else if let Some(result) = registry_agents() {
                                    match result {
                                        Ok(_) => rsx! {
                                            p { class: "agent-panel__hint", "{registry_name} is not available in the ACP registry for this platform." }
                                        },
                                        Err(err) => rsx! {
                                            p { class: "agent-panel__hint", "Failed to load ACP registry: {err}" }
                                        },
                                    }
                                } else {
                                    p { class: "agent-panel__hint", "{acp_registry_loading_text()}" }
                                }
                                    }
                                }
                            }
                        },
                        AgentSetupMode::Custom => rsx! {
                            div { class: "agent-panel__section",
                                div { class: "agent-panel__section-header",
                                    div { class: "agent-panel__section-copy",
                                        h4 { class: "agent-panel__section-title", "Custom ACP agent" }
                                        p { class: "agent-panel__hint", "Connect any ACP-compatible binary over stdio." }
                                    }
                                    span { class: "agent-panel__badge", "stdio" }
                                }
                                div { class: "field",
                                    label { class: "field__label", "ACP command" }
                                    input {
                                        class: "input",
                                        value: "{state.launch.command}",
                                        placeholder: "path/to/acp-agent",
                                        oninput: move |event| {
                                            let value = event.value();
                                            panel_state.with_mut(|state| state.launch.command = value);
                                        }
                                    }
                                }
                                div { class: "field",
                                    label { class: "field__label", "Arguments" }
                                    input {
                                        class: "input",
                                        value: "{state.launch.args}",
                                        placeholder: "--arg value",
                                        oninput: move |event| {
                                            let value = event.value();
                                            panel_state.with_mut(|state| state.launch.args = value);
                                        }
                                    }
                                }
                                div { class: "field",
                                    label { class: "field__label", "Working directory" }
                                    input {
                                        class: "input",
                                        value: "{state.launch.cwd}",
                                        oninput: move |event| {
                                            let value = event.value();
                                            panel_state.with_mut(|state| state.launch.cwd = value);
                                        }
                                    }
                                }
                                button {
                                    class: "button button--primary",
                                    disabled: state.busy || state.launch.command.trim().is_empty(),
                                    onclick: move |_| {
                                        let mut launch = panel_state().launch.clone();
                                        launch.env.clear();
                                        panel_state.with_mut(|state| {
                                            state.busy = true;
                                            state.status = "Connecting to ACP agent...".to_string();
                                        });
                                        spawn(async move {
                                            match services::connect_acp_agent(launch).await {
                                                Ok(connection) => {
                                                    panel_state.with_mut(|state| {
                                                        state::apply_connected(state, connection);
                                                    });
                                                }
                                                Err(err) => {
                                                    panel_state.with_mut(|state| {
                                                        state.busy = false;
                                                        state.connected = false;
                                                        state.connection = None;
                                                        state.status = err.clone();
                                                        push_message(state, AcpMessageKind::Error, err);
                                                    });
                                                    chat_revision += 1;
                                                }
                                            }
                                        });
                                    },
                                    "Connect Agent"
                                }
                            }
                        },
                    }}
                }
            }
            }
        }
    }
}

/// Shared Copy / Insert / Run action row for a SQL snippet rendered in
/// the transcript (code cards, artifacts, and extracted candidates).
/// The caller wraps these buttons in the container that matches the
/// surrounding block (`code-actions`, `artifact-actions`, or
/// `message-actions`).
#[component]
fn SqlActionButtons(
    panel_state: Signal<AcpPanelState>,
    store: TabStore,
    chat_revision: Signal<u64>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    sql: String,
    #[props(default = "Run SQL")] run_label: &'static str,
) -> Element {
    rsx! {
        button {
            class: "button button--ghost button--small",
            onclick: {
                let sql = sql.clone();
                move |event| {
                    event.stop_propagation();
                    copy_text_to_clipboard(panel_state, sql.clone(), "SQL");
                }
            },
            "Copy SQL"
        }
        button {
            class: "button button--ghost button--small",
            onclick: {
                let sql = sql.clone();
                move |event| {
                    event.stop_propagation();
                    insert_sql_into_editor(panel_state, store, store.active_tab_id, sql.clone());
                }
            },
            "Insert SQL"
        }
        button {
            class: "button button--primary button--small",
            disabled: !can_execute_agent_sql(
                &sql,
                allow_agent_read_sql_run(),
                allow_agent_write_sql_run(),
            ),
            onclick: {
                let sql = sql.clone();
                move |event| {
                    event.stop_propagation();
                    execute_agent_sql_request(
                        panel_state,
                        store,
                        store.active_tab_id,
                        chat_revision,
                        sql.clone(),
                        AgentSqlExecutionMode::Manual,
                        true,
                    );
                }
            },
            {run_label.to_string()}
        }
    }
}
