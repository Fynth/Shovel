use agent_client_protocol::{self as acp, Client as _, SessionUpdate};
use futures_util::StreamExt;
use models::{AcpLaunchRequest, AcpOllamaConfig, AgentSpecialist};
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

const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434/api";

/// System prompts for specialist agent types.
/// Injected as the first system message when a specialist is active.
const SQL_EXPERT_SYSTEM_PROMPT: &str = "You are a SQL expert. Optimize queries, explain execution plans, suggest indexes, and help with complex joins. Focus on performance and correctness.";
const DATA_ANALYST_SYSTEM_PROMPT: &str = "You are a data analyst. Find trends, anomalies, and patterns in data. Generate insights, calculate statistics, and explain findings clearly.";
const SCHEMA_ARCHITECT_SYSTEM_PROMPT: &str = "You are a schema architect. Design migrations, normalize tables, define constraints and indexes. Focus on data integrity and scalability.";

/// Returns the specialist-specific system prompt.
fn get_specialist_system_prompt(specialist: AgentSpecialist) -> &'static str {
    match specialist {
        AgentSpecialist::SqlExpert => SQL_EXPERT_SYSTEM_PROMPT,
        AgentSpecialist::DataAnalyst => DATA_ANALYST_SYSTEM_PROMPT,
        AgentSpecialist::SchemaArchitect => SCHEMA_ARCHITECT_SYSTEM_PROMPT,
    }
}

#[derive(Clone, Debug)]
pub struct EmbeddedOllamaAgentConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelSummary>,
}

#[derive(Clone, Debug, Deserialize)]
struct OllamaModelSummary {
    model: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChatMessage>,
    #[serde(default)]
    _done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct OllamaSession {
    history: Vec<OllamaChatMessage>,
}

type SessionUpdates = mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>;
type Sessions = Arc<Mutex<HashMap<String, OllamaSession>>>;
type CancelFlags = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;
type SpecialistUsage = Arc<Mutex<HashMap<String, u64>>>;
type HandoffContext = Arc<Mutex<Vec<OllamaChatMessage>>>;

struct OllamaAgent {
    client: reqwest::Client,
    config: EmbeddedOllamaAgentConfig,
    session_updates: SessionUpdates,
    next_session_id: Cell<u64>,
    sessions: Sessions,
    cancel_flags: CancelFlags,
    active_specialist: Cell<Option<AgentSpecialist>>,
    specialist_usage: SpecialistUsage,
    handoff_context: HandoffContext,
}

impl OllamaAgent {
    fn new(
        config: EmbeddedOllamaAgentConfig,
        session_updates: SessionUpdates,
    ) -> Result<Self, String> {
        Ok(Self {
            client: reqwest::Client::builder()
                .build()
                .map_err(|err| format!("Failed to build Ollama HTTP client: {err}"))?,
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

    async fn send_session_text(&self, session_id: &str, text: String) -> Result<(), acp::Error> {
        if text.is_empty() {
            return Ok(());
        }

        let (tx, rx) = oneshot::channel();
        self.session_updates
            .send((
                acp::SessionNotification::new(
                    session_id.to_string(),
                    SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(text.into())),
                ),
                tx,
            ))
            .map_err(|_| acp::Error::internal_error().data("ACP session update channel closed"))?;

        rx.await
            .map_err(|_| acp::Error::internal_error().data("ACP session update ack dropped"))?;
        Ok(())
    }

    fn set_specialist(&self, specialist: AgentSpecialist) {
        let prev = self.active_specialist.get();
        if prev.is_some()
            && prev != Some(specialist)
            && let Ok(sessions) = self.sessions.lock()
        {
            let mut all_history = Vec::new();
            for session in sessions.values() {
                all_history.extend(session.history.iter().cloned());
            }
            if let Ok(mut handoff) = self.handoff_context.lock() {
                *handoff = all_history;
            }
        }
        self.active_specialist.set(Some(specialist));
    }

    fn active_specialist(&self) -> Option<AgentSpecialist> {
        self.active_specialist.get()
    }

    fn specialist_usage(&self) -> HashMap<String, u64> {
        self.specialist_usage
            .lock()
            .map(|usage| usage.clone())
            .unwrap_or_default()
    }

    fn handoff_to(&self, specialist: AgentSpecialist) {
        self.set_specialist(specialist);
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for OllamaAgent {
    async fn initialize(
        &self,
        _args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        let title = format!("Ollama ACP Bridge ({})", self.config.model);
        Ok(
            acp::InitializeResponse::new(acp::ProtocolVersion::V1).agent_info(
                acp::Implementation::new("shovel-ollama", env!("CARGO_PKG_VERSION")).title(title),
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
        let session_id = format!("ollama-{}", self.next_session_id.get());
        self.next_session_id.set(self.next_session_id.get() + 1);
        self.sessions
            .lock()
            .map_err(|_| {
                acp::Error::internal_error().data("Ollama session registry lock poisoned")
            })?
            .insert(
                session_id.clone(),
                OllamaSession {
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
            return Err(acp::Error::invalid_params().data("Ollama model is empty"));
        }

        let prior_history = {
            let sessions = self.sessions.lock().map_err(|_| {
                acp::Error::internal_error().data("Ollama session registry lock poisoned")
            })?;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| acp::Error::invalid_params().data("Unknown Ollama ACP session"))?;
            session.history.clone()
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut cancel_flags = self.cancel_flags.lock().map_err(|_| {
                acp::Error::internal_error().data("Ollama cancel registry lock poisoned")
            })?;
            cancel_flags.insert(session_id.clone(), Arc::clone(&cancel_flag));
        }

        let user_message = OllamaChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
        };
        let mut request_messages = Vec::new();

        if let Some(specialist) = self.active_specialist.get() {
            request_messages.push(OllamaChatMessage {
                role: "system".to_string(),
                content: get_specialist_system_prompt(specialist).to_string(),
            });

            if let Ok(handoff) = self.handoff_context.lock()
                && !handoff.is_empty()
            {
                let context_summary: String = handoff
                    .iter()
                    .filter(|msg| msg.role != "system")
                    .map(|msg| format!("{}: {}", msg.role, msg.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !context_summary.is_empty() {
                    request_messages.push(OllamaChatMessage {
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

        let request = OllamaChatRequest {
            model: model.to_string(),
            messages: request_messages,
            stream: true,
        };

        let response = self
            .client
            .post(format!(
                "{}/chat",
                normalize_base_url(&self.config.base_url)
            ))
            .headers(ollama_headers(self.config.api_key.as_deref())?)
            .json(&request)
            .send()
            .await
            .map_err(|err| {
                acp::Error::internal_error().data(format!("Ollama request failed: {err}"))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            cleanup_cancel_flag(&self.cancel_flags, &session_id);
            return Err(acp::Error::internal_error()
                .data(format!("Ollama returned {status}: {}", body.trim())));
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
                acp::Error::internal_error().data(format!("Ollama stream failed: {err}"))
            })?;

            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(newline_index) = buffer.find('\n') {
                let line = buffer[..newline_index].trim().to_string();
                buffer.drain(..=newline_index);

                if line.is_empty() {
                    continue;
                }

                let stream_chunk: OllamaChatChunk = serde_json::from_str(&line).map_err(|err| {
                    acp::Error::internal_error()
                        .data(format!("Failed to parse Ollama stream chunk: {err}"))
                })?;

                if let Some(error) = stream_chunk.error {
                    cleanup_cancel_flag(&self.cancel_flags, &session_id);
                    return Err(acp::Error::internal_error().data(error));
                }

                if let Some(message) = stream_chunk.message
                    && !message.content.is_empty()
                {
                    assistant_reply.push_str(&message.content);
                    self.send_session_text(&session_id, message.content).await?;
                }
            }
        }

        if !buffer.trim().is_empty() {
            let stream_chunk: OllamaChatChunk =
                serde_json::from_str(buffer.trim()).map_err(|err| {
                    acp::Error::internal_error()
                        .data(format!("Failed to parse final Ollama stream chunk: {err}"))
                })?;

            if let Some(error) = stream_chunk.error {
                cleanup_cancel_flag(&self.cancel_flags, &session_id);
                return Err(acp::Error::internal_error().data(error));
            }

            if let Some(message) = stream_chunk.message
                && !message.content.is_empty()
            {
                assistant_reply.push_str(&message.content);
                self.send_session_text(&session_id, message.content).await?;
            }
        }

        cleanup_cancel_flag(&self.cancel_flags, &session_id);

        let assistant_reply = assistant_reply.trim().to_string();
        self.sessions
            .lock()
            .map_err(|_| {
                acp::Error::internal_error().data("Ollama session registry lock poisoned")
            })?
            .entry(session_id)
            .and_modify(|session| {
                session.history.push(user_message);
                if !assistant_reply.is_empty() {
                    session.history.push(OllamaChatMessage {
                        role: "assistant".to_string(),
                        content: assistant_reply.clone(),
                    });
                }
            });

        if let Some(specialist) = self.active_specialist.get() {
            eprintln!(
                "[ollama::specialist] Response from: {}",
                specialist.variant_name()
            );
        }

        Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        let session_id = args.session_id.to_string();
        if let Some(cancel_flag) = self
            .cancel_flags
            .lock()
            .map_err(|_| acp::Error::internal_error().data("Ollama cancel registry lock poisoned"))?
            .get(&session_id)
            .cloned()
        {
            cancel_flag.store(true, Ordering::Relaxed);
        }

        Ok(())
    }
}

/// Adapter that wraps [`OllamaAgent`] with specialist-specific behavior.
///
/// Injects specialist system prompts based on [`AgentSpecialist`] type,
/// tracks usage for telemetry, and manages handoff context between
/// specialist transitions by preserving conversation history.
pub struct OllamaSpecialistAdapter {
    agent: OllamaAgent,
}

impl OllamaSpecialistAdapter {
    pub fn with_specialist(
        config: EmbeddedOllamaAgentConfig,
        session_updates: SessionUpdates,
        specialist: AgentSpecialist,
    ) -> Result<Self, String> {
        let agent = OllamaAgent::new(config, session_updates)?;
        agent.set_specialist(specialist);
        Ok(Self { agent })
    }

    pub fn set_specialist(&self, specialist: AgentSpecialist) {
        self.agent.set_specialist(specialist);
    }

    pub fn active_specialist(&self) -> Option<AgentSpecialist> {
        self.agent.active_specialist()
    }

    pub fn specialist_usage(&self) -> HashMap<String, u64> {
        self.agent.specialist_usage()
    }

    pub fn handoff_to(&self, specialist: AgentSpecialist) {
        self.agent.handoff_to(specialist);
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for OllamaSpecialistAdapter {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        self.agent.initialize(args).await
    }

    async fn authenticate(
        &self,
        args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        self.agent.authenticate(args).await
    }

    async fn new_session(
        &self,
        args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        self.agent.new_session(args).await
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        self.agent.prompt(args).await
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        self.agent.cancel(args).await
    }
}

pub fn build_embedded_ollama_launch(
    cwd: String,
    config: AcpOllamaConfig,
) -> Result<AcpLaunchRequest, String> {
    let model = config.model.trim();
    if model.is_empty() {
        return Err("Ollama model is required".to_string());
    }

    let command = std::env::current_exe()
        .map_err(|err| format!("Failed to resolve Shovel executable path: {err}"))?
        .display()
        .to_string();

    let mut args = vec![
        "acp-agent".to_string(),
        "ollama".to_string(),
        "--base-url".to_string(),
        normalize_base_url(&config.base_url),
        "--model".to_string(),
        model.to_string(),
    ];

    if !config.api_key.trim().is_empty() {
        args.push("--api-key".to_string());
        args.push(config.api_key.trim().to_string());
    }

    Ok(AcpLaunchRequest {
        command,
        args: shell_join(&args),
        cwd,
        env: Vec::new(),
    })
}

pub async fn load_ollama_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    let response = reqwest::Client::new()
        .get(format!("{}/tags", normalize_base_url(&base_url)))
        .headers(ollama_headers(api_key.as_deref()).map_err(|err| err.to_string())?)
        .send()
        .await
        .map_err(|err| format!("Failed to query Ollama models: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {status}: {}", body.trim()));
    }

    let payload: OllamaTagsResponse = response
        .json()
        .await
        .map_err(|err| format!("Failed to parse Ollama models: {err}"))?;

    let mut models = payload
        .models
        .into_iter()
        .filter_map(|entry| entry.model.or(entry.name))
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

pub fn run_embedded_ollama_agent(config: EmbeddedOllamaAgentConfig) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Failed to start ACP runtime: {err}"))?;

    let local_set = tokio::task::LocalSet::new();
    runtime
        .block_on(local_set.run_until(async move { run_embedded_ollama_agent_async(config).await }))
}

async fn run_embedded_ollama_agent_async(config: EmbeddedOllamaAgentConfig) -> Result<(), String> {
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    let (session_updates, mut session_update_rx) = mpsc::unbounded_channel();
    let agent = OllamaAgent::new(config, session_updates)?;
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
    result.map_err(|err| format!("ACP agent I/O failed: {err}"))
}

fn cleanup_cancel_flag(cancel_flags: &CancelFlags, session_id: &str) {
    if let Ok(mut cancel_flags) = cancel_flags.lock() {
        cancel_flags.remove(session_id);
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
    let normalized = if trimmed.is_empty() {
        DEFAULT_OLLAMA_BASE_URL.to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    };

    if normalized.ends_with("/api") {
        normalized
    } else {
        format!("{normalized}/api")
    }
}

fn ollama_headers(api_key: Option<&str>) -> Result<HeaderMap, acp::Error> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    if let Some(api_key) = api_key
        && !api_key.trim().is_empty()
    {
        let value =
            HeaderValue::from_str(&format!("Bearer {}", api_key.trim())).map_err(|err| {
                acp::Error::invalid_params().data(format!("Invalid Ollama API key header: {err}"))
            })?;
        headers.insert(AUTHORIZATION, value);
    }

    Ok(headers)
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
        .bytes()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' | b':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
