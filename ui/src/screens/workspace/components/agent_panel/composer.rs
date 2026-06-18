use dioxus::prelude::*;
use models::{AcpPanelState, QueryTabState};

use super::{
    prompt::{active_editor_error, active_editor_sql},
    requests::{
        send_chat_prompt_request,
        send_sql_error_fix_request,
        send_sql_explanation_request,
        send_sql_generation_request,
        send_sql_plan_request,
    },
};

#[component]
pub(super) fn AgentComposer(
    panel_state: Signal<AcpPanelState>,
    tabs: Signal<Vec<QueryTabState>>,
    active_tab_id: Signal<u64>,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    busy: bool,
    connection_label: String,
    reset_key: String,
) -> Element {
    let mut prompt_draft = use_signal(String::new);
    let mut prompt_reset_revision = use_signal(|| 0_u64);
    let reset_effect_key = reset_key.clone();

    use_effect(move || {
        let _ = reset_effect_key.as_str();
        prompt_draft.set(String::new());
    });

    let prompt_is_empty = prompt_draft().trim().is_empty();
    let active_sql = active_editor_sql(tabs, active_tab_id());
    let has_active_sql = active_sql.is_some();
    let has_explainable_sql = active_sql
        .as_deref()
        .is_some_and(services::is_read_only_sql);
    let has_active_error = active_editor_error(tabs, active_tab_id()).is_some();
    let enter_chat_label = connection_label.clone();
    let generate_sql_label = connection_label.clone();
    let chat_label = connection_label.clone();
    let explain_plan_label = connection_label.clone();
    let explain_sql_label = connection_label.clone();
    let fix_sql_label = connection_label.clone();
    let prompt_textarea_key = format!("{reset_key}-{}", prompt_reset_revision());

    rsx! {
        div { class: "agent-panel__composer",
            div { class: "agent-panel__permissions",
                label { class: "agent-panel__permission-toggle",
                    input {
                        r#type: "checkbox",
                        checked: allow_agent_db_read(),
                        onchange: move |event| {
                            allow_agent_db_read.set(event.checked());
                        }
                    }
                    span { "Allow ACP to read database context" }
                }
                label { class: "agent-panel__permission-toggle",
                    input {
                        r#type: "checkbox",
                        checked: allow_agent_read_sql_run(),
                        onchange: move |event| {
                            allow_agent_read_sql_run.set(event.checked());
                        }
                    }
                    span { "Allow ACP to execute read-only SQL in the active tab" }
                }
                label { class: "agent-panel__permission-toggle",
                    input {
                        r#type: "checkbox",
                        checked: allow_agent_write_sql_run(),
                        onchange: move |event| {
                            allow_agent_write_sql_run.set(event.checked());
                        }
                    }
                    span { "Allow ACP to execute write SQL in the active tab" }
                }
                label { class: "agent-panel__permission-toggle",
                    input {
                        r#type: "checkbox",
                        checked: allow_agent_tool_run(),
                        onchange: move |event| {
                            allow_agent_tool_run.set(event.checked());
                        }
                    }
                    span { "Allow ACP tools and code execution" }
                }
            }
            textarea {
                key: "{prompt_textarea_key}",
                class: "input agent-panel__prompt",
                rows: 5,
                initial_value: "{prompt_draft}",
                placeholder: "For example: show active users created today",
                oninput: move |event| prompt_draft.set(event.value()),
                onkeydown: move |event| {
                    if event.key() != Key::Enter
                        || event.modifiers().contains(Modifiers::SHIFT)
                    {
                        return;
                    }
                    event.prevent_default();
                    let prompt = prompt_draft();
                    if prompt.trim().is_empty() || panel_state().busy {
                        return;
                    }
                    prompt_draft.set(String::new());
                    prompt_reset_revision += 1;
                    send_chat_prompt_request(
                        panel_state,
                        tabs,
                        active_tab_id(),
                        enter_chat_label.clone(),
                        chat_revision,
                        allow_agent_db_read(),
                        prompt,
                        prompt_draft,
                    );
                }
            }
            div { class: "agent-panel__composer-actions",
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !allow_agent_read_sql_run() || !has_explainable_sql,
                    onclick: move |_| {
                        send_sql_plan_request(
                            panel_state,
                            tabs,
                            active_tab_id(),
                            explain_plan_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            allow_agent_read_sql_run(),
                        );
                    },
                    "Explain Plan"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !has_active_sql,
                    onclick: move |_| {
                        send_sql_explanation_request(
                            panel_state,
                            tabs,
                            active_tab_id(),
                            explain_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                        );
                    },
                    "Explain SQL"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !has_active_error,
                    onclick: move |_| {
                        send_sql_error_fix_request(
                            panel_state,
                            tabs,
                            active_tab_id(),
                            fix_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                        );
                    },
                    "Fix SQL Error"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || prompt_is_empty,
                    onclick: move |_| {
                        let prompt = prompt_draft();
                        if prompt.trim().is_empty() || panel_state().busy {
                            return;
                        }
                        prompt_draft.set(String::new());
                        prompt_reset_revision += 1;
                        send_sql_generation_request(
                            panel_state,
                            tabs,
                            active_tab_id(),
                            generate_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            prompt,
                            Some(prompt_draft),
                            true,
                        );
                    },
                    "Generate SQL"
                }
                button {
                    class: "button button--primary button--small",
                    disabled: busy || prompt_is_empty,
                    onclick: move |_| {
                        let prompt = prompt_draft();
                        if prompt.trim().is_empty() || panel_state().busy {
                            return;
                        }
                        prompt_draft.set(String::new());
                        prompt_reset_revision += 1;
                        send_chat_prompt_request(
                            panel_state,
                            tabs,
                            active_tab_id(),
                            chat_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            prompt,
                            prompt_draft,
                        );
                    },
                    "Send"
                }
            }
        }
    }
}
