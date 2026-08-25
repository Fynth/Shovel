use crate::ChatArtifact;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpLaunchRequest {
    pub command: String,
    pub args: String,
    pub cwd: String,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpOllamaConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpConnectionInfo {
    pub agent_name: String,
    pub session_id: String,
    pub protocol_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpRegistryAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcpMessageKind {
    User,
    Agent,
    Thought,
    Tool,
    System,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpUiMessage {
    pub id: u64,
    pub kind: AcpMessageKind,
    pub text: String,
    pub created_at: i64,
    pub artifact: Option<ChatArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpPermissionOption {
    pub option_id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpPermissionRequest {
    pub request_id: u64,
    pub tool_summary: String,
    pub options: Vec<AcpPermissionOption>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcpEvent {
    Connected(AcpConnectionInfo),
    Status(String),
    Message { kind: AcpMessageKind, text: String },
    PermissionRequested(AcpPermissionRequest),
    PromptStarted,
    PromptFinished { stop_reason: String },
    Error(String),
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpPanelState {
    pub launch: AcpLaunchRequest,
    pub ollama: AcpOllamaConfig,
    pub prompt: String,
    pub status: String,
    pub connected: bool,
    pub busy: bool,
    pub pending_sql_insert: bool,
    pub suppress_transcript: bool,
    pub hidden_agent_response: String,
    pub optimizer_response: String,
    pub optimizer_request_active: bool,
    pub connection: Option<AcpConnectionInfo>,
    pub messages: Vec<AcpUiMessage>,
    pub next_message_id: u64,
    pub pending_permission: Option<AcpPermissionRequest>,
}

impl AcpPanelState {
    #[must_use]
    pub fn new(launch: AcpLaunchRequest, ollama: AcpOllamaConfig) -> Self {
        Self {
            launch,
            ollama,
            prompt: String::new(),
            status: "ACP agent is disconnected.".to_string(),
            connected: false,
            busy: false,
            pending_sql_insert: false,
            suppress_transcript: false,
            hidden_agent_response: String::new(),
            optimizer_response: String::new(),
            optimizer_request_active: false,
            connection: None,
            messages: Vec::new(),
            next_message_id: 1,
            pending_permission: None,
        }
    }
}
