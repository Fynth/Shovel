use dioxus::prelude::*;
use models::{
    AcpConnectionInfo,
    AcpMessageKind,
    AcpPanelState,
    ActiveModel,
    AiProviderKind,
    AppUiSettings,
    DeepSeekSettings,
    OllamaSettings,
    builtin_providers,
    is_native_http_ready,
    provider_kind,
};

use super::{
    messages::acp_registry_preparing_text,
    state::{apply_connected, push_message},
};

pub(super) const NATIVE_HTTP_PROTOCOL: &str = "native-http";

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

    let settings = crate::app_state::APP_UI_SETTINGS();
    if let Some(active) = settings.ai_catalog.active.clone() {
        let key = settings.lm_api_key(&active.provider);
        if is_native_http_ready(&active.provider, &key) {
            let label = native_provider_label(&settings, &active.provider);
            apply_native_connected_signal(panel_state, label);
            return Ok(());
        }
        match provider_kind(&active.provider) {
            Some(AiProviderKind::Acp) if active.provider == "acp:codex" => {
                return connect_registry_agent(
                    panel_state,
                    chat_revision,
                    CODEX_REGISTRY_AGENT_ID,
                    "Codex CLI",
                )
                .await;
            }
            Some(AiProviderKind::Acp) => {
                return ensure_opencode_connected(panel_state, chat_revision).await;
            }
            Some(AiProviderKind::NativeHttp) | None => {}
        }
    }

    let deepseek_key = settings.lm_api_key("deepseek");
    let deepseek_key = if deepseek_key.trim().is_empty() {
        deepseek.api_key.clone()
    } else {
        deepseek_key
    };
    if deepseek.enabled && is_native_http_ready("deepseek", &deepseek_key) {
        return connect_embedded_deepseek(panel_state, chat_revision, deepseek);
    }

    let ollama_key = settings.lm_api_key("ollama");
    let ollama_key = if ollama_key.trim().is_empty() {
        ollama.api_key.clone()
    } else {
        ollama_key
    };
    if ollama.enabled
        && is_native_http_ready("ollama", &ollama_key)
        && !ollama.model.trim().is_empty()
    {
        return connect_embedded_ollama(panel_state, chat_revision, ollama);
    }

    Err("No language model is ready. Add an API key in Settings.".to_string())
}

pub(crate) fn connect_embedded_ollama(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    ollama: OllamaSettings,
) -> Result<(), String> {
    connect_native_http(
        panel_state,
        chat_revision,
        NativeConnectArgs {
            provider: "ollama",
            label: "Ollama",
            api_key: &ollama.api_key,
            model: &ollama.model,
            base_url: &ollama.base_url,
            thinking_enabled: false,
            reasoning_effort: "medium",
        },
    )
}

pub(crate) fn connect_embedded_deepseek(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    deepseek: DeepSeekSettings,
) -> Result<(), String> {
    connect_native_http(
        panel_state,
        chat_revision,
        NativeConnectArgs {
            provider: "deepseek",
            label: "DeepSeek",
            api_key: &deepseek.api_key,
            model: &deepseek.model,
            base_url: &deepseek.base_url,
            thinking_enabled: deepseek.thinking_enabled,
            reasoning_effort: &deepseek.reasoning_effort,
        },
    )
}

struct NativeConnectArgs<'a> {
    provider: &'a str,
    label: &'a str,
    api_key: &'a str,
    model: &'a str,
    base_url: &'a str,
    thinking_enabled: bool,
    reasoning_effort: &'a str,
}

fn connect_native_http(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    args: NativeConnectArgs<'_>,
) -> Result<(), String> {
    let NativeConnectArgs {
        provider,
        label,
        api_key,
        model,
        base_url,
        thinking_enabled,
        reasoning_effort,
    } = args;
    let key = {
        let from_lm = crate::app_state::lm_api_key(provider);
        if from_lm.trim().is_empty() {
            api_key.to_string()
        } else {
            from_lm
        }
    };
    if !is_native_http_ready(provider, &key) {
        let err = format!("Add an API key to connect {label}.");
        panel_state.with_mut(|state| {
            state.busy = false;
            state.status = err.clone();
            push_message(state, AcpMessageKind::Error, err.clone());
        });
        chat_revision += 1;
        return Err(err);
    }
    if provider == "ollama" && model.trim().is_empty() {
        let err = "Ollama model is required.".to_string();
        panel_state.with_mut(|state| {
            state.busy = false;
            state.status = err.clone();
            push_message(state, AcpMessageKind::Error, err.clone());
        });
        chat_revision += 1;
        return Err(err);
    }

    crate::app_state::update_ui_settings(|current| {
        current.lm_keys.insert(provider.to_string(), key.clone());
        current.ai_catalog.active = Some(ActiveModel {
            provider: provider.to_string(),
            model: model.to_string(),
        });
        let over = current
            .ai_catalog
            .overrides
            .entry(provider.to_string())
            .or_default();
        if !base_url.trim().is_empty() {
            over.base_url = base_url.to_string();
        }
        over.thinking_enabled = thinking_enabled;
        over.reasoning_effort = reasoning_effort.to_string();
        over.enabled = true;
    });

    apply_native_connected_signal(panel_state, label.to_string());
    Ok(())
}

pub(super) fn apply_native_connected_signal(
    mut panel_state: Signal<AcpPanelState>,
    agent_name: String,
) {
    panel_state.with_mut(|state| {
        apply_native_connected(state, agent_name);
    });
}

pub(super) fn apply_native_connected(state: &mut AcpPanelState, agent_name: String) {
    apply_connected(
        state,
        AcpConnectionInfo {
            agent_name,
            session_id: String::new(),
            protocol_version: NATIVE_HTTP_PROTOCOL.to_string(),
        },
    );
}

pub(super) fn is_native_http_connection(state: &AcpPanelState) -> bool {
    state
        .connection
        .as_ref()
        .is_some_and(|connection| connection.protocol_version == NATIVE_HTTP_PROTOCOL)
}

fn native_provider_label(settings: &AppUiSettings, provider: &str) -> String {
    if let Some(spec) = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
    {
        return spec.label.to_string();
    }
    settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
        .map(|custom| custom.name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| provider.to_string())
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
