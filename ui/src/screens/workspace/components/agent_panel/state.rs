use models::{
    AcpConnectionInfo, AcpEvent, AcpLaunchRequest, AcpMessageKind, AcpOllamaConfig, AcpPanelState,
    AcpUiMessage, ChatArtifact,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn default_acp_panel_state() -> AcpPanelState {
    let cwd = std::env::var("SHOVEL_ACP_CWD")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            services::acp_workspace_root()
                .ok()
                .map(|path| path.display().to_string())
        })
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
        })
        .unwrap_or_else(|| ".".to_string());

    AcpPanelState::new(
        AcpLaunchRequest {
            command: std::env::var("SHOVEL_ACP_COMMAND").unwrap_or_default(),
            args: std::env::var("SHOVEL_ACP_ARGS").unwrap_or_default(),
            cwd,
            env: Vec::new(),
        },
        AcpOllamaConfig {
            base_url: std::env::var("SHOVEL_OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434/api".to_string()),
            model: std::env::var("SHOVEL_OLLAMA_MODEL").unwrap_or_default(),
            api_key: std::env::var("OLLAMA_API_KEY").unwrap_or_default(),
        },
    )
}

pub(crate) fn apply_acp_events(state: &mut AcpPanelState, events: Vec<AcpEvent>) {
    for event in events {
        match event {
            AcpEvent::Connected(connection) => {
                apply_connected(state, connection);
            }
            AcpEvent::Status(status) => {
                state.status = status;
            }
            AcpEvent::Message { kind, text } => {
                if state.suppress_transcript {
                    buffer_hidden_message(state, kind, text);
                } else {
                    push_or_append_message(state, kind, text);
                }
            }
            AcpEvent::PermissionRequested(request) => {
                state.pending_permission = Some(request);
                state.busy = true;
                state.status = "ACP agent is waiting for permission.".to_string();
            }
            AcpEvent::PromptStarted => {
                state.busy = true;
                state.status = "Agent is working...".to_string();
            }
            AcpEvent::PromptFinished { stop_reason } => {
                state.busy = false;
                state.pending_permission = None;
                state.suppress_transcript = false;
                state.status = prompt_finished_status(&stop_reason);
                state
                    .messages
                    .retain(|message| !matches!(message.kind, AcpMessageKind::Thought));
            }
            AcpEvent::Error(error) => {
                let suppress_transcript = state.suppress_transcript;
                state.busy = false;
                state.pending_permission = None;
                state.pending_sql_insert = false;
                state.suppress_transcript = false;
                state.hidden_agent_response.clear();
                state.status = error.clone();
                state
                    .messages
                    .retain(|message| !matches!(message.kind, AcpMessageKind::Thought));
                if !suppress_transcript {
                    push_message(state, AcpMessageKind::Error, error);
                }
            }
            AcpEvent::Disconnected => {
                state.busy = false;
                state.connected = false;
                state.pending_sql_insert = false;
                state.suppress_transcript = false;
                state.hidden_agent_response.clear();
                state.connection = None;
                state.pending_permission = None;
                state.status = "ACP agent disconnected.".to_string();
                state
                    .messages
                    .retain(|message| !matches!(message.kind, AcpMessageKind::Thought));
            }
        }
    }
}

fn prompt_finished_status(stop_reason: &str) -> String {
    let stop_reason = stop_reason.trim();
    if stop_reason.is_empty() || stop_reason == "EndTurn" {
        "Ready".to_string()
    } else {
        format!("Finished: {stop_reason}")
    }
}

pub(super) fn apply_connected(state: &mut AcpPanelState, connection: AcpConnectionInfo) {
    state.connected = true;
    state.busy = false;
    state.pending_sql_insert = false;
    state.suppress_transcript = false;
    state.hidden_agent_response.clear();
    state.connection = Some(connection.clone());
    state.pending_permission = None;
    state.messages.retain(|message| {
        !matches!(message.kind, AcpMessageKind::System)
            || !message.text.starts_with("Connected to ")
    });
    state.status = format!("Connected to {}", connection.agent_name);
}

fn push_or_append_message(state: &mut AcpPanelState, kind: AcpMessageKind, text: String) {
    if text.is_empty() && !matches!(kind, AcpMessageKind::Tool | AcpMessageKind::Thought) {
        return;
    }

    if matches!(kind, AcpMessageKind::Thought) {
        if state
            .messages
            .last()
            .is_some_and(|last| matches!(last.kind, AcpMessageKind::Thought))
        {
            return;
        }

        push_message(state, kind, String::new());
        return;
    }

    if let Some(last) = state.messages.last_mut()
        && last.kind == kind
    {
        if matches!(kind, AcpMessageKind::Tool) {
            return;
        }
        last.text.push_str(&text);
        return;
    }

    if matches!(kind, AcpMessageKind::Tool) {
        push_message(state, kind, "🛠".to_string());
    } else {
        push_message(state, kind, text);
    }
}

fn buffer_hidden_message(state: &mut AcpPanelState, kind: AcpMessageKind, text: String) {
    if matches!(kind, AcpMessageKind::Agent) && !text.is_empty() {
        state.hidden_agent_response.push_str(&text);
    }
}

pub(super) fn push_message(state: &mut AcpPanelState, kind: AcpMessageKind, text: String) {
    push_message_with_artifact(state, kind, text, None);
}

pub(crate) fn push_message_with_artifact(
    state: &mut AcpPanelState,
    kind: AcpMessageKind,
    text: String,
    artifact: Option<ChatArtifact>,
) {
    let id = state.next_message_id;
    state.next_message_id += 1;
    state.messages.push(AcpUiMessage {
        id,
        kind,
        text,
        created_at: unix_timestamp(),
        artifact,
    });
}

pub(crate) fn replace_messages(state: &mut AcpPanelState, messages: Vec<AcpUiMessage>) {
    let messages = messages
        .into_iter()
        .filter(|message| !matches!(message.kind, AcpMessageKind::Thought))
        .collect::<Vec<_>>();
    state.next_message_id = next_message_id(&messages);
    state.messages = messages;
}

pub(super) fn message_kind_label(kind: &AcpMessageKind) -> &'static str {
    match kind {
        AcpMessageKind::User => "You",
        AcpMessageKind::Agent => "Agent",
        AcpMessageKind::Thought => "Working",
        AcpMessageKind::Tool => "Tool",
        AcpMessageKind::System => "Status",
        AcpMessageKind::Error => "Error",
    }
}

pub(super) fn message_kind_class(kind: &AcpMessageKind) -> &'static str {
    match kind {
        AcpMessageKind::User => "user",
        AcpMessageKind::Agent => "agent",
        AcpMessageKind::Thought => "thought",
        AcpMessageKind::Tool => "tool",
        AcpMessageKind::System => "system",
        AcpMessageKind::Error => "error",
    }
}

pub(super) fn message_kind_avatar(kind: &AcpMessageKind) -> &'static str {
    match kind {
        AcpMessageKind::User => "👤",
        AcpMessageKind::Agent => "🤖",
        AcpMessageKind::Thought => "💭",
        AcpMessageKind::Tool => "🔧",
        AcpMessageKind::System => "ℹ️",
        AcpMessageKind::Error => "⚠️",
    }
}

pub(super) fn permission_button_class(kind: &str) -> &'static str {
    if kind.contains("Allow") {
        "button button--primary button--small"
    } else {
        "button button--ghost button--small"
    }
}

fn next_message_id(messages: &[AcpUiMessage]) -> u64 {
    messages.iter().map(|message| message.id).max().unwrap_or(0) + 1
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{apply_acp_events, prompt_finished_status, replace_messages};
    use models::{
        AcpEvent, AcpLaunchRequest, AcpMessageKind, AcpOllamaConfig, AcpPanelState, AcpUiMessage,
    };

    fn test_state() -> AcpPanelState {
        AcpPanelState::new(
            AcpLaunchRequest {
                command: String::new(),
                args: String::new(),
                cwd: ".".to_string(),
                env: Vec::new(),
            },
            AcpOllamaConfig {
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            },
        )
    }

    #[test]
    fn normalizes_end_turn_prompt_status() {
        assert_eq!(prompt_finished_status("EndTurn"), "Ready");
    }

    #[test]
    fn keeps_other_stop_reasons_compact() {
        assert_eq!(prompt_finished_status("MaxTokens"), "Finished: MaxTokens");
    }

    #[test]
    fn suppresses_transcript_messages_during_hidden_prompt() {
        let mut state = test_state();
        state.suppress_transcript = true;

        apply_acp_events(
            &mut state,
            vec![
                AcpEvent::Message {
                    kind: AcpMessageKind::Agent,
                    text: "SELECT ".to_string(),
                },
                AcpEvent::Message {
                    kind: AcpMessageKind::Agent,
                    text: "1".to_string(),
                },
            ],
        );

        assert!(state.messages.is_empty());
        assert_eq!(state.hidden_agent_response, "SELECT 1");
    }

    #[test]
    fn hidden_prompt_errors_do_not_create_visible_messages() {
        let mut state = test_state();
        state.suppress_transcript = true;

        apply_acp_events(&mut state, vec![AcpEvent::Error("boom".to_string())]);

        assert!(state.messages.is_empty());
        assert!(!state.suppress_transcript);
        assert!(state.hidden_agent_response.is_empty());
    }

    #[test]
    fn replace_messages_drops_stale_working_entries() {
        let mut state = test_state();

        replace_messages(
            &mut state,
            vec![
                AcpUiMessage {
                    id: 1,
                    kind: AcpMessageKind::Thought,
                    text: String::new(),
                    created_at: 1,
                    artifact: None,
                },
                AcpUiMessage {
                    id: 2,
                    kind: AcpMessageKind::Agent,
                    text: "SELECT 1".to_string(),
                    created_at: 2,
                    artifact: None,
                },
            ],
        );

        assert_eq!(state.messages.len(), 1);
        assert!(matches!(state.messages[0].kind, AcpMessageKind::Agent));
    }
}
