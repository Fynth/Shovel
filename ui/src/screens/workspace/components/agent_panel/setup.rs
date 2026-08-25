use dioxus::prelude::*;
use models::{AcpMessageKind, AcpPanelState, DeepSeekSettings, OllamaSettings};

use super::{
    messages::acp_registry_preparing_text,
    state::{apply_connected, push_message},
};

const OPENCODE_REGISTRY_AGENT_ID: &str = "opencode";
const CODEX_REGISTRY_AGENT_ID: &str = "codex-acp";

async fn connect_registry_agent(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    agent_id: &str,
    agent_name: &str,
) -> Result<(), String> {
    let cwd = panel_state().launch.cwd.clone();
    panel_state.with_mut(|state| {
        state.busy = true;
        state.status = acp_registry_preparing_text(agent_name);
    });

    let launch = match services::install_acp_registry_agent(agent_id.to_string(), cwd).await {
        Ok(launch) => launch,
        Err(err) => {
            panel_state.with_mut(|state| {
                state.busy = false;
                state.status = err.clone();
                push_message(state, AcpMessageKind::Error, err.clone());
            });
            chat_revision += 1;
            return Err(err);
        }
    };

    panel_state.with_mut(|state| {
        state.launch = launch.clone();
        state.busy = true;
        state.status = format!("Connecting to {agent_name}...");
    });

    match services::connect_acp_agent(launch).await {
        Ok(connection) => {
            panel_state.with_mut(|state| {
                apply_connected(state, connection);
            });
            Ok(())
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
            Err(err)
        }
    }
}

pub(crate) async fn ensure_opencode_connected(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
) -> Result<(), String> {
    if panel_state().connected {
        return Ok(());
    }

    if panel_state().busy {
        let status = panel_state().status.trim().to_string();
        return Err(if status.is_empty() {
            "ACP agent is busy.".to_string()
        } else {
            status
        });
    }

    connect_registry_agent(
        panel_state,
        chat_revision,
        OPENCODE_REGISTRY_AGENT_ID,
        "OpenCode",
    )
    .await
}

pub(crate) async fn ensure_default_sql_agent_connected(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    deepseek: DeepSeekSettings,
    ollama: OllamaSettings,
) -> Result<(), String> {
    if panel_state().connected {
        return Ok(());
    }

    if panel_state().busy {
        let status = panel_state().status.trim().to_string();
        return Err(if status.is_empty() {
            "ACP agent is busy.".to_string()
        } else {
            status
        });
    }

    if deepseek.enabled && !deepseek.api_key.trim().is_empty() {
        connect_embedded_deepseek(panel_state, chat_revision, deepseek).await
    } else if ollama.enabled && !ollama.model.trim().is_empty() {
        connect_embedded_ollama(panel_state, chat_revision, ollama).await
    } else {
        ensure_opencode_connected(panel_state, chat_revision).await
    }
}

pub(crate) async fn connect_embedded_ollama(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    ollama: OllamaSettings,
) -> Result<(), String> {
    let cwd = panel_state().launch.cwd.clone();
    panel_state.with_mut(|state| {
        state.busy = true;
        state.status = format!("Connecting to Ollama model {}...", ollama.model.trim());
    });

    let config = models::AcpOllamaConfig {
        base_url: ollama.base_url.clone(),
        model: ollama.model.clone(),
        api_key: ollama.api_key.clone(),
    };
    let launch = match services::build_embedded_ollama_launch(cwd, config) {
        Ok(launch) => launch,
        Err(err) => {
            panel_state.with_mut(|state| {
                state.busy = false;
                state.status = err.clone();
                push_message(state, AcpMessageKind::Error, err.clone());
            });
            chat_revision += 1;
            return Err(err);
        }
    };

    panel_state.with_mut(|state| {
        state.launch = launch.clone();
        state.busy = true;
        state.status = format!(
            "Launching embedded Ollama ACP bridge for {}...",
            ollama.model.trim()
        );
    });

    match services::connect_acp_agent(launch).await {
        Ok(connection) => {
            panel_state.with_mut(|state| {
                apply_connected(state, connection);
            });
            Ok(())
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
            Err(err)
        }
    }
}

pub(crate) async fn connect_embedded_deepseek(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    deepseek: DeepSeekSettings,
) -> Result<(), String> {
    let cwd = panel_state().launch.cwd.clone();
    panel_state.with_mut(|state| {
        state.busy = true;
        state.status = format!("Connecting to DeepSeek model {}...", deepseek.model.trim());
    });

    let launch = match services::build_embedded_deepseek_launch(cwd, deepseek.clone()) {
        Ok(launch) => launch,
        Err(err) => {
            panel_state.with_mut(|state| {
                state.busy = false;
                state.status = err.clone();
                push_message(state, AcpMessageKind::Error, err.clone());
            });
            chat_revision += 1;
            return Err(err);
        }
    };

    panel_state.with_mut(|state| {
        state.launch = launch.clone();
        state.busy = true;
        state.status = format!(
            "Launching embedded DeepSeek ACP bridge for {}...",
            deepseek.model
        );
    });

    match services::connect_acp_agent(launch).await {
        Ok(connection) => {
            panel_state.with_mut(|state| {
                apply_connected(state, connection);
            });
            Ok(())
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
            Err(err)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentSetupMode {
    DeepSeek,
    Ollama,
    OpenCode,
    Codex,
    Custom,
}

impl AgentSetupMode {
    pub(super) const ALL: [Self; 5] = [
        Self::DeepSeek,
        Self::Ollama,
        Self::OpenCode,
        Self::Codex,
        Self::Custom,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Ollama => "Ollama",
            Self::OpenCode => "OpenCode",
            Self::Codex => "Codex",
            Self::Custom => "Custom",
        }
    }

    pub(super) fn meta(self) -> &'static str {
        match self {
            Self::DeepSeek => "API key",
            Self::Ollama => "Embedded",
            Self::OpenCode | Self::Codex => "Registry",
            Self::Custom => "stdio",
        }
    }

    pub(super) fn registry_agent_id(self) -> Option<&'static str> {
        match self {
            Self::OpenCode => Some(OPENCODE_REGISTRY_AGENT_ID),
            Self::Codex => Some(CODEX_REGISTRY_AGENT_ID),
            Self::DeepSeek | Self::Ollama | Self::Custom => None,
        }
    }

    pub(super) fn registry_name(self) -> Option<&'static str> {
        match self {
            Self::OpenCode => Some("OpenCode"),
            Self::Codex => Some("Codex CLI"),
            Self::DeepSeek | Self::Ollama | Self::Custom => None,
        }
    }

    pub(super) fn registry_hint(self) -> Option<&'static str> {
        match self {
            Self::OpenCode => Some("OpenCode agent."),
            Self::Codex => Some("Codex CLI agent."),
            Self::DeepSeek | Self::Ollama | Self::Custom => None,
        }
    }
}

pub(super) fn setup_mode_button_class(
    mode: AgentSetupMode,
    active_mode: AgentSetupMode,
) -> &'static str {
    if mode == active_mode {
        "button button--ghost button--active agent-panel__mode-button"
    } else {
        "button button--ghost agent-panel__mode-button"
    }
}
