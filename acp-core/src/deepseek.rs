use agent_client_protocol::{self as acp, Client as _, SessionUpdate};
use futures_util::StreamExt;
use models::{AcpLaunchRequest, AgentSpecialist, DeepSeekSettings};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

const SQL_EXPERT_SYSTEM_PROMPT: &str = "You are a SQL expert embedded in Shovel, a desktop database client. Optimize queries, explain execution plans, suggest indexes, and help with complex joins. Focus on correctness, database-specific SQL, and safe operations.";
const DATA_ANALYST_SYSTEM_PROMPT: &str = "You are a data analyst embedded in Shovel, a desktop database client. Find trends, anomalies, and patterns in data. Generate insights, calculate statistics, and explain findings clearly.";
const SCHEMA_ARCHITECT_SYSTEM_PROMPT: &str = "You are a schema architect embedded in Shovel, a desktop database client. Design migrations, normalize tables, define constraints and indexes. Focus on data integrity and scalability.";

#[derive(Clone, Debug)]
pub struct EmbeddedDeepSeekAgentConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, Serialize)]
struct DeepSeekChatRequest {
    model: String,
    messages: Vec<DeepSeekChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<DeepSeekThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeepSeekChatMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Serialize)]
struct DeepSeekThinking {
    r#type: String,
}

#[derive(Clone, Debug, Deserialize)]
struct DeepSeekChatChunk {
    #[serde(default)]
    choices: Vec<DeepSeekChoice>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct DeepSeekChoice {
    delta: DeepSeekDelta,
}

#[derive(Clone, Debug, Deserialize)]
struct DeepSeekDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
}

#[derive(Clone, Debug)]
struct DeepSeekSession {
    history: Vec<DeepSeekChatMessage>,
}

type SessionUpdates = mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>;
type Sessions = Arc<Mutex<HashMap<String, DeepSeekSession>>>;
type CancelFlags = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;
type SpecialistUsage = Arc<Mutex<HashMap<String, u64>>>;
type HandoffContext = Arc<Mutex<Vec<DeepSeekChatMessage>>>;

struct DeepSeekAgent {
    client: reqwest::Client,
    config: EmbeddedDeepSeekAgentConfig,
    session_updates: SessionUpdates,
    next_session_id: Cell<u64>,
    sessions: Sessions,
    cancel_flags: CancelFlags,
    active_specialist: Cell<Option<AgentSpecialist>>,
    specialist_usage: SpecialistUsage,
    handoff_context: HandoffContext,
}

impl DeepSeekAgent {
    fn new(
        config: EmbeddedDeepSeekAgentConfig,
        session_updates: SessionUpdates,
    ) -> Result<Self, String> {
        Ok(Self {
            client: reqwest::Client::builder()
                .build()
                .map_err(|err| format!("Failed to build DeepSeek HTTP client: {err}"))?,
            config,
            session_updates,
            next_session_id: Cell::new(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            active_specialist: Cell::new(None),
            specialist_usage: Arc::new(Mutex::new(HashMap::new())),
            handoff_context: Arc::new(Mutex::new(Vec::new())),
        })
    }

    async fn send_session_text(
        &self,
        session_id: &str,
        update: SessionUpdate,
    ) -> Result<(), acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.session_updates
            .send((
                acp::SessionNotification::new(session_id.to_string(), update),
                tx,
            ))
            .map_err(|_| acp::Error::internal_error().data("ACP session update channel closed"))?;

        rx.await
            .map_err(|_| acp::Error::internal_error().data("ACP session update ack dropped"))?;
        Ok(())
    }

    async fn send_agent_chunk(&self, session_id: &str, text: String) -> Result<(), acp::Error> {
        if text.is_empty() {
            return Ok(());
        }
        self.send_session_text(
            session_id,
            SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(text.into())),
        )
        .await
    }

    async fn send_thought_chunk(&self, session_id: &str, text: String) -> Result<(), acp::Error> {
        if text.is_empty() {
            return Ok(());
        }
        self.send_session_text(
            session_id,
            SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(text.into())),
        )
        .await
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for DeepSeekAgent {
    async fn initialize(
        &self,
        _args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        let title = format!("DeepSeek ACP Bridge ({})", self.config.model);
        Ok(
            acp::InitializeResponse::new(acp::ProtocolVersion::V1).agent_info(
                acp::Implementation::new("shovel-deepseek", env!("CARGO_PKG_VERSION")).title(title),
            ),
        )
    }

    async fn authenticate(
        &self,
        _args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let id = self.next_session_id.get();
        self.next_session_id.set(id + 1);
        let session_id = format!("deepseek-{id}");
        self.sessions
            .lock()
            .map_err(|_| acp::Error::internal_error().data("DeepSeek session lock poisoned"))?
            .insert(
                session_id.clone(),
                DeepSeekSession {
                    history: Vec::new(),
                },
            );
        Ok(acp::NewSessionResponse::new(session_id))
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        let session_id = args.session_id.to_string();
        let prompt = prompt_blocks_to_text(&args.prompt);
        if prompt.trim().is_empty() {
            return Err(acp::Error::invalid_params().data("Prompt is empty"));
        }

        let model = self.config.model.trim();
        if model.is_empty() {
            return Err(acp::Error::invalid_params().data("DeepSeek model is empty"));
        }

        let api_key = self.config.api_key.trim();
        if api_key.is_empty() {
            return Err(acp::Error::invalid_params().data("DeepSeek API key is required"));
        }

        let prior_history = {
            let sessions = self.sessions.lock().map_err(|_| {
                acp::Error::internal_error().data("DeepSeek session registry lock poisoned")
            })?;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| acp::Error::invalid_params().data("Unknown DeepSeek session"))?;
            session.history.clone()
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut cancel_flags = self.cancel_flags.lock().map_err(|_| {
                acp::Error::internal_error().data("DeepSeek cancel registry lock poisoned")
            })?;
            cancel_flags.insert(session_id.clone(), Arc::clone(&cancel_flag));
        }

        let user_message = DeepSeekChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
        };
        let mut request_messages = Vec::new();

        if let Some(specialist) = self.active_specialist.get() {
            request_messages.push(DeepSeekChatMessage {
                role: "system".to_string(),
                content: get_specialist_system_prompt(specialist).to_string(),
            });

            if let Ok(handoff) = self.handoff_context.lock()
                && !handoff.is_empty()
            {
                let context_summary = handoff
                    .iter()
                    .filter(|msg| msg.role != "system")
                    .map(|msg| format!("{}: {}", msg.role, msg.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !context_summary.is_empty() {
                    request_messages.push(DeepSeekChatMessage {
                        role: "system".to_string(),
                        content: format!(
                            "[Handoff context from previous specialist]\n{context_summary}"
                        ),
                    });
                }
            }

            if let Ok(mut usage) = self.specialist_usage.lock() {
                let key = specialist.variant_name().to_string();
                *usage.entry(key).or_insert(0) += 1;
            }
        }

        request_messages.extend(prior_history.clone());
        request_messages.push(user_message.clone());

        let request = DeepSeekChatRequest {
            model: model.to_string(),
            messages: request_messages,
            stream: true,
            thinking: self.config.thinking_enabled.then(|| DeepSeekThinking {
                r#type: "enabled".to_string(),
            }),
            reasoning_effort: normalized_reasoning_effort(&self.config.reasoning_effort),
        };

        let response = self
            .client
            .post(format!(
                "{}/v1/chat/completions",
                normalize_base_url(&self.config.base_url)
            ))
            .headers(deepseek_headers(api_key)?)
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                acp::Error::internal_error().data(format!("DeepSeek request failed: {err}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            cleanup_cancel_flag(&self.cancel_flags, &session_id);
            return Err(acp::Error::internal_error()
                .data(format!("DeepSeek returned {status}: {}", body.trim())));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut assistant_reply = String::new();

        while let Some(chunk) = stream.next().await {
            if cancel_flag.load(Ordering::Relaxed) {
                cleanup_cancel_flag(&self.cancel_flags, &session_id);
                return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
            }

            let chunk = chunk.map_err(|err| {
                acp::Error::internal_error().data(format!("DeepSeek stream failed: {err}"))
            })?;

            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline_index) = buffer.find('\n') {
                let line = buffer[..newline_index].trim().to_string();
                buffer.drain(..=newline_index);
                self.process_sse_line(&session_id, &line, &mut assistant_reply)
                    .await?;
            }
        }

        if !buffer.trim().is_empty() {
            self.process_sse_line(&session_id, buffer.trim(), &mut assistant_reply)
                .await?;
        }

        cleanup_cancel_flag(&self.cancel_flags, &session_id);
        let assistant_reply = assistant_reply.trim().to_string();
        self.sessions
            .lock()
            .map_err(|_| {
                acp::Error::internal_error().data("DeepSeek session registry lock poisoned")
            })?
            .entry(session_id)
            .and_modify(|session| {
                session.history.push(user_message);
                if !assistant_reply.is_empty() {
                    session.history.push(DeepSeekChatMessage {
                        role: "assistant".to_string(),
                        content: assistant_reply.clone(),
                    });
                }
            });

        Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        let session_id = args.session_id.to_string();
        if let Ok(cancel_flags) = self.cancel_flags.lock()
            && let Some(flag) = cancel_flags.get(&session_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

impl DeepSeekAgent {
    async fn process_sse_line(
        &self,
        session_id: &str,
        line: &str,
        assistant_reply: &mut String,
    ) -> Result<(), acp::Error> {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return Ok(());
        };
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }

        let chunk: DeepSeekChatChunk = serde_json::from_str(data).map_err(|err| {
            acp::Error::internal_error()
                .data(format!("Failed to parse DeepSeek stream chunk: {err}"))
        })?;

        // Check for API-level errors embedded in the stream.
        if let Some(ref error) = chunk.error {
            return Err(
                acp::Error::internal_error().data(format!("DeepSeek stream error: {error}"))
            );
        }

        for choice in chunk.choices {
            if let Some(reasoning) = choice.delta.reasoning_content
                && !reasoning.is_empty()
            {
                self.send_thought_chunk(session_id, reasoning).await?;
            }
            if let Some(content) = choice.delta.content
                && !content.is_empty()
            {
                assistant_reply.push_str(&content);
                self.send_agent_chunk(session_id, content).await?;
            }
        }

        Ok(())
    }
}

pub fn build_embedded_deepseek_launch(
    cwd: String,
    config: DeepSeekSettings,
) -> Result<AcpLaunchRequest, String> {
    let api_key = config.api_key.trim();
    if api_key.is_empty() {
        return Err("DeepSeek API key is required".to_string());
    }

    let model = config.model.trim();
    if model.is_empty() {
        return Err("DeepSeek model is required".to_string());
    }

    let command = std::env::current_exe()
        .map_err(|err| format!("Failed to resolve Shovel executable path: {err}"))?
        .display()
        .to_string();

    let args = vec![
        "acp-agent".to_string(),
        "deepseek".to_string(),
        "--base-url".to_string(),
        normalize_base_url(&config.base_url),
        "--model".to_string(),
        model.to_string(),
        "--thinking".to_string(),
        if config.thinking_enabled {
            "enabled".to_string()
        } else {
            "disabled".to_string()
        },
        "--reasoning-effort".to_string(),
        normalize_reasoning_effort_value(&config.reasoning_effort).to_string(),
    ];

    Ok(AcpLaunchRequest {
        command,
        args: shell_join(&args),
        cwd,
        env: vec![("DEEPSEEK_API_KEY".to_string(), api_key.to_string())],
    })
}

pub fn run_embedded_deepseek_agent(config: EmbeddedDeepSeekAgentConfig) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Failed to start ACP runtime: {err}"))?;

    let local_set = tokio::task::LocalSet::new();
    runtime.block_on(
        local_set.run_until(async move { run_embedded_deepseek_agent_async(config).await }),
    )
}

async fn run_embedded_deepseek_agent_async(
    config: EmbeddedDeepSeekAgentConfig,
) -> Result<(), String> {
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    let (session_updates, mut session_update_rx) = mpsc::unbounded_channel();
    let agent = DeepSeekAgent::new(config, session_updates)?;
    let (conn, io_task) = acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
        tokio::task::spawn_local(fut);
    });

    tokio::task::spawn_local(async move {
        while let Some((session_notification, tx)) = session_update_rx.recv().await {
            let result: acp::Result<()> = conn.session_notification(session_notification).await;
            if result.is_err() {
                break;
            }
            let _ = tx.send(());
        }
    });

    let result: acp::Result<()> = io_task.await;
    result.map_err(|err| format!("DeepSeek ACP agent I/O failed: {err}"))
}

fn cleanup_cancel_flag(cancel_flags: &CancelFlags, session_id: &str) {
    if let Ok(mut cancel_flags) = cancel_flags.lock() {
        cancel_flags.remove(session_id);
    }
}

fn get_specialist_system_prompt(specialist: AgentSpecialist) -> &'static str {
    match specialist {
        AgentSpecialist::SqlExpert => SQL_EXPERT_SYSTEM_PROMPT,
        AgentSpecialist::DataAnalyst => DATA_ANALYST_SYSTEM_PROMPT,
        AgentSpecialist::SchemaArchitect => SCHEMA_ARCHITECT_SYSTEM_PROMPT,
    }
}

fn prompt_blocks_to_text(blocks: &[acp::ContentBlock]) -> String {
    blocks
        .iter()
        .map(content_block_to_text)
        .filter(|content| !content.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn content_block_to_text(block: &acp::ContentBlock) -> String {
    match block {
        acp::ContentBlock::Text(text) => text.text.clone(),
        acp::ContentBlock::ResourceLink(link) => format!("Resource: {}", link.uri),
        acp::ContentBlock::Resource(resource) => {
            serde_json::to_string_pretty(resource).unwrap_or_else(|_| "<resource>".to_string())
        }
        acp::ContentBlock::Image(_) => "<image>".to_string(),
        acp::ContentBlock::Audio(_) => "<audio>".to_string(),
        _ => "<content>".to_string(),
    }
}

fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        DEFAULT_DEEPSEEK_BASE_URL.to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

fn deepseek_headers(api_key: &str) -> Result<HeaderMap, acp::Error> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let value = HeaderValue::from_str(&format!("Bearer {}", api_key.trim())).map_err(|err| {
        acp::Error::invalid_params().data(format!("Invalid DeepSeek API key header: {err}"))
    })?;
    headers.insert(AUTHORIZATION, value);
    Ok(headers)
}

fn normalized_reasoning_effort(value: &str) -> Option<String> {
    Some(normalize_reasoning_effort_value(value).to_string())
}

fn normalize_reasoning_effort_value(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{build_embedded_deepseek_launch, normalize_reasoning_effort_value};
    use models::DeepSeekSettings;

    #[test]
    fn deepseek_launch_passes_api_key_through_env_not_args() {
        let settings = DeepSeekSettings {
            enabled: true,
            api_key: "sk-secret".to_string(),
            ..DeepSeekSettings::default()
        };

        let launch = build_embedded_deepseek_launch(".".to_string(), settings).expect("launch");

        assert!(!launch.args.contains("sk-secret"));
        assert_eq!(
            launch.env,
            vec![("DEEPSEEK_API_KEY".to_string(), "sk-secret".to_string())]
        );
    }

    #[test]
    fn normalizes_unknown_reasoning_effort_to_medium() {
        assert_eq!(normalize_reasoning_effort_value("high"), "high");
        assert_eq!(normalize_reasoning_effort_value("unknown"), "medium");
    }
}
