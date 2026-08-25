use dioxus::prelude::*;
use models::{AcpPanelState, QueryTabState};

use super::requests::send_chat_prompt_request;

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
    let enter_chat_label = connection_label.clone();
    let chat_label = connection_label.clone();
    let prompt_textarea_key = format!("{reset_key}-{}", prompt_reset_revision());

    // Focus the composer textarea when the workspace dispatcher bumps
    // the global focus-request counter (Ctrl+Shift+M). Mirrors the SQL
    // editor's focus wiring.
    let focus_target_id = prompt_textarea_key.clone();
    use_effect(move || {
        let _ = crate::app_state::APP_FOCUS_AGENT_COMPOSER_REQUEST();
        let _ = document::eval(&format!(
            r#"
            (() => {{
                const el = document.getElementById({id:?});
                if (el) {{
                    el.focus();
                }}
            }})()
            "#,
            id = focus_target_id
        ));
    });

    rsx! {
        div { class: "agent-panel__composer",
            textarea {
                key: "{prompt_textarea_key}",
                class: "input agent-panel__prompt",
                rows: 1,
                initial_value: "{prompt_draft}",
                placeholder: "Ask the agent…",
                oninput: move |event| prompt_draft.set(event.value()),
                onkeydown: move |event| {
                    // Send on bare Enter (chat-style) or Ctrl+Enter
                    // (editor-style). Shift+Enter inserts a newline.
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
                    class: "button button--primary button--small agent-panel__send",
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
                    title: if prompt_is_empty {
                        "Type a prompt to send to the agent"
                    } else {
                        "Send prompt to the agent (Enter)"
                    },
                    if busy {
                        span { class: "agent-panel__streaming-caret", aria_hidden: "true" }
                        " Sending…"
                    } else {
                        "Send"
                    }
                }
            }
        }
    }
}
