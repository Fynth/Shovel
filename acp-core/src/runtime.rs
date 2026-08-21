use agent_client_protocol::{
    self as acp, Agent as _, ContentBlock, ContentChunk, RequestPermissionOutcome,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
};
use models::{
    AcpConnectionInfo, AcpEvent, AcpLaunchRequest, AcpMessageKind, AcpPermissionOption,
    AcpPermissionRequest, AgentRoutingRequest, AgentRoutingResponse, AgentSpecialist,
};

use crate::agents::AgentCoordinator;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};
use tokio::{
    io::AsyncReadExt,
    process::Child,
    sync::{Mutex as AsyncMutex, mpsc as tokio_mpsc, oneshot},
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct AcpRuntimeHandle {
    command_tx: tokio_mpsc::UnboundedSender<AcpCommand>,
    event_rx: mpsc::Receiver<AcpEvent>,
    permission_registry: PermissionRegistry,
    terminal_registry: TerminalRegistry,
    thread: thread::JoinHandle<()>,
    active_specialist: Option<AgentSpecialist>,
    coordinator: AgentCoordinator,
    execution_history: Vec<String>,
}

enum AcpCommand {
    Prompt(String),
    Cancel,
    Disconnect,
}

struct BridgeClient {
    event_tx: mpsc::Sender<AcpEvent>,
    permission_registry: PermissionRegistry,
    terminal_registry: TerminalRegistry,
    workspace_root: PathBuf,
}

type PermissionRegistry = Arc<PermissionRegistryInner>;
type TerminalRegistry = Arc<TerminalRegistryInner>;

struct PermissionRegistryInner {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<RequestPermissionResponse>>>,
}

struct TerminalRegistryInner {
    next_id: AtomicU64,
    terminals: Mutex<HashMap<String, Arc<TerminalState>>>,
}

struct TerminalState {
    child: AsyncMutex<Child>,
    output: Mutex<String>,
    output_limit: Option<usize>,
    truncated: Mutex<bool>,
    exit_status: Mutex<Option<acp::TerminalExitStatus>>,
}

impl PermissionRegistryInner {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn insert(&self, sender: oneshot::Sender<RequestPermissionResponse>) -> Result<u64, String> {
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending
            .lock()
            .map_err(|_| "ACP permission registry lock poisoned".to_string())?
            .insert(request_id, sender);
        Ok(request_id)
    }

    fn take(
        &self,
        request_id: u64,
    ) -> Result<Option<oneshot::Sender<RequestPermissionResponse>>, String> {
        self.pending
            .lock()
            .map_err(|_| "ACP permission registry lock poisoned".to_string())
            .map(|mut pending| pending.remove(&request_id))
    }
}

impl TerminalRegistryInner {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            terminals: Mutex::new(HashMap::new()),
        }
    }

    fn next_terminal_id(&self) -> String {
        format!("term-{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn insert(&self, terminal_id: String, terminal: Arc<TerminalState>) -> Result<(), String> {
        self.terminals
            .lock()
            .map_err(|_| "ACP terminal registry lock poisoned".to_string())?
            .insert(terminal_id, terminal);
        Ok(())
    }

    fn get(&self, terminal_id: &str) -> Result<Option<Arc<TerminalState>>, String> {
        self.terminals
            .lock()
            .map_err(|_| "ACP terminal registry lock poisoned".to_string())
            .map(|terminals| terminals.get(terminal_id).cloned())
    }

    fn take(&self, terminal_id: &str) -> Result<Option<Arc<TerminalState>>, String> {
        self.terminals
            .lock()
            .map_err(|_| "ACP terminal registry lock poisoned".to_string())
            .map(|mut terminals| terminals.remove(terminal_id))
    }

    fn ids(&self) -> Result<Vec<String>, String> {
        self.terminals
            .lock()
            .map_err(|_| "ACP terminal registry lock poisoned".to_string())
            .map(|terminals| terminals.keys().cloned().collect())
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for BridgeClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<RequestPermissionResponse> {
        let (response_tx, response_rx) = oneshot::channel();
        let request_id = match self.permission_registry.insert(response_tx) {
            Ok(request_id) => request_id,
            Err(err) => {
                let _ = self.event_tx.send(AcpEvent::Error(err));
                return Ok(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                ));
            }
        };

        let permission_request = AcpPermissionRequest {
            request_id,
            tool_summary: format!("{:?}", args.tool_call),
            options: args
                .options
                .iter()
                .map(|option| AcpPermissionOption {
                    option_id: option.option_id.to_string(),
                    label: option.name.clone(),
                    kind: format!("{:?}", option.kind),
                })
                .collect(),
        };

        let _ = self
            .event_tx
            .send(AcpEvent::PermissionRequested(permission_request));

        match response_rx.await {
            Ok(response) => Ok(response),
            Err(_) => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            )),
        }
    }

    async fn session_notification(&self, args: SessionNotification) -> acp::Result<()> {
        match args.update {
            SessionUpdate::UserMessageChunk(chunk) => {
                send_chunk(&self.event_tx, AcpMessageKind::User, chunk);
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                send_chunk(&self.event_tx, AcpMessageKind::Agent, chunk);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                send_chunk(&self.event_tx, AcpMessageKind::Thought, chunk);
            }
            SessionUpdate::ToolCall(tool_call) => {
                let _ = self.event_tx.send(AcpEvent::Message {
                    kind: AcpMessageKind::Tool,
                    text: format!("Tool call: {tool_call:?}"),
                });
            }
            SessionUpdate::ToolCallUpdate(tool_update) => {
                let _ = self.event_tx.send(AcpEvent::Message {
                    kind: AcpMessageKind::Tool,
                    text: format!("Tool update: {tool_update:?}"),
                });
            }
            SessionUpdate::Plan(plan) => {
                let _ = self.event_tx.send(AcpEvent::Message {
                    kind: AcpMessageKind::System,
                    text: format!("Plan: {plan:?}"),
                });
            }
            SessionUpdate::SessionInfoUpdate(_) => {}
            SessionUpdate::AvailableCommandsUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
            | SessionUpdate::ConfigOptionUpdate(_) => {}
            _ => {}
        }

        Ok(())
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        let path = resolve_workspace_path(&self.workspace_root, &args.path, true)
            .map_err(|err| acp::Error::invalid_params().data(err))?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(acp::Error::into_internal_error)?;
        }

        tokio::fs::write(&path, args.content)
            .await
            .map_err(acp::Error::into_internal_error)?;

        let _ = self.event_tx.send(AcpEvent::Message {
            kind: AcpMessageKind::Tool,
            text: format!("ACP wrote file {}", path.display()),
        });

        Ok(acp::WriteTextFileResponse::new())
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        let path = resolve_workspace_path(&self.workspace_root, &args.path, false)
            .map_err(|err| acp::Error::invalid_params().data(err))?;
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(acp::Error::into_internal_error)?;
        let content = apply_read_window(content, args.line, args.limit);

        let _ = self.event_tx.send(AcpEvent::Message {
            kind: AcpMessageKind::Tool,
            text: format!("ACP read file {}", path.display()),
        });

        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        let cwd = match &args.cwd {
            Some(cwd) => resolve_workspace_path(&self.workspace_root, cwd, false)
                .map_err(|err| acp::Error::invalid_params().data(err))?,
            None => self.workspace_root.clone(),
        };
        if !cwd.is_dir() {
            return Err(acp::Error::invalid_params().data(format!(
                "ACP terminal cwd must be a directory: {}",
                cwd.display()
            )));
        }

        let mut command = tokio::process::Command::new(&args.command);
        command
            .args(&args.args)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        apply_hidden_process_flags(&mut command);
        for env_var in args.env {
            command.env(env_var.name, env_var.value);
        }

        let mut child = command.spawn().map_err(acp::Error::into_internal_error)?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let terminal_id = self.terminal_registry.next_terminal_id();
        let terminal = Arc::new(TerminalState {
            child: AsyncMutex::new(child),
            output: Mutex::new(String::new()),
            output_limit: args.output_byte_limit.map(|limit| limit as usize),
            truncated: Mutex::new(false),
            exit_status: Mutex::new(None),
        });

        self.terminal_registry
            .insert(terminal_id.clone(), Arc::clone(&terminal))
            .map_err(|err| acp::Error::internal_error().data(err))?;

        if let Some(stdout) = stdout {
            tokio::task::spawn_local(read_terminal_stream(
                Arc::clone(&terminal),
                stdout,
                self.event_tx.clone(),
                terminal_id.clone(),
                "stdout",
            ));
        }
        if let Some(stderr) = stderr {
            tokio::task::spawn_local(read_terminal_stream(
                Arc::clone(&terminal),
                stderr,
                self.event_tx.clone(),
                terminal_id.clone(),
                "stderr",
            ));
        }

        let _ = self.event_tx.send(AcpEvent::Message {
            kind: AcpMessageKind::Tool,
            text: format!(
                "ACP created terminal {terminal_id} in {} for `{}`",
                cwd.display(),
                args.command
            ),
        });

        Ok(acp::CreateTerminalResponse::new(terminal_id))
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        let terminal = self
            .terminal_registry
            .get(&args.terminal_id.to_string())
            .map_err(|err| acp::Error::internal_error().data(err))?
            .ok_or_else(|| acp::Error::invalid_params().data("Unknown ACP terminal".to_string()))?;

        update_terminal_exit_status(&terminal)
            .await
            .map_err(acp::Error::into_internal_error)?;

        let output = terminal
            .output
            .lock()
            .map_err(|_| acp::Error::internal_error().data("ACP terminal output lock poisoned"))?
            .clone();
        let truncated = *terminal.truncated.lock().map_err(|_| {
            acp::Error::internal_error().data("ACP terminal truncation lock poisoned")
        })?;
        let exit_status = terminal
            .exit_status
            .lock()
            .map_err(|_| {
                acp::Error::internal_error().data("ACP terminal exit status lock poisoned")
            })?
            .clone();

        Ok(acp::TerminalOutputResponse::new(output, truncated).exit_status(exit_status))
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        let Some(terminal) = self
            .terminal_registry
            .take(&args.terminal_id.to_string())
            .map_err(|err| acp::Error::internal_error().data(err))?
        else {
            return Ok(acp::ReleaseTerminalResponse::new());
        };

        terminate_terminal(&terminal)
            .await
            .map_err(acp::Error::into_internal_error)?;

        let _ = self.event_tx.send(AcpEvent::Message {
            kind: AcpMessageKind::Tool,
            text: format!("ACP released terminal {}", args.terminal_id),
        });

        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        let terminal = self
            .terminal_registry
            .get(&args.terminal_id.to_string())
            .map_err(|err| acp::Error::internal_error().data(err))?
            .ok_or_else(|| acp::Error::invalid_params().data("Unknown ACP terminal".to_string()))?;

        let exit_status = wait_for_terminal_exit_status(&terminal)
            .await
            .map_err(acp::Error::into_internal_error)?;

        Ok(acp::WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal(
        &self,
        args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        let terminal = self
            .terminal_registry
            .get(&args.terminal_id.to_string())
            .map_err(|err| acp::Error::internal_error().data(err))?
            .ok_or_else(|| acp::Error::invalid_params().data("Unknown ACP terminal".to_string()))?;

        terminate_terminal(&terminal)
            .await
            .map_err(acp::Error::into_internal_error)?;

        let _ = self.event_tx.send(AcpEvent::Message {
            kind: AcpMessageKind::Tool,
            text: format!("ACP killed terminal {}", args.terminal_id),
        });

        Ok(acp::KillTerminalResponse::new())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

static ACP_RUNTIME: OnceLock<Mutex<Option<AcpRuntimeHandle>>> = OnceLock::new();

fn runtime_slot() -> &'static Mutex<Option<AcpRuntimeHandle>> {
    ACP_RUNTIME.get_or_init(|| Mutex::new(None))
}

pub async fn connect_acp_agent(request: AcpLaunchRequest) -> Result<AcpConnectionInfo, String> {
    disconnect_acp_agent()?;

    let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = oneshot::channel();
    let worker_request = request.clone();
    let permission_registry = Arc::new(PermissionRegistryInner::new());
    let worker_permission_registry = Arc::clone(&permission_registry);
    let terminal_registry = Arc::new(TerminalRegistryInner::new());
    let worker_terminal_registry = Arc::clone(&terminal_registry);

    let thread = thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                let _ = ready_tx.send(Err(format!("Failed to start ACP runtime: {err}")));
                return;
            }
        };

        let local_set = tokio::task::LocalSet::new();
        local_set.block_on(&runtime, async move {
            run_acp_worker(
                worker_request,
                command_rx,
                event_tx,
                ready_tx,
                worker_permission_registry,
                worker_terminal_registry,
            )
            .await;
        });
    });

    let connection = ready_rx
        .await
        .map_err(|_| "ACP worker stopped before initialization finished".to_string())??;

    *runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())? = Some(AcpRuntimeHandle {
        command_tx,
        event_rx,
        permission_registry,
        terminal_registry,
        thread,
        active_specialist: None,
        coordinator: AgentCoordinator::new(),
        execution_history: Vec::new(),
    });

    Ok(connection)
}

pub fn send_acp_prompt(prompt: String) -> Result<(), String> {
    send_command(AcpCommand::Prompt(prompt))
}

/// Routes an ACP request through the agent coordinator to determine the appropriate specialist.
/// Returns routing decision with confidence score and reasoning.
pub fn route_acp_request(request: AgentRoutingRequest) -> Result<AgentRoutingResponse, String> {
    let mut slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let Some(handle) = slot.as_mut() else {
        return Err("ACP agent is not connected".to_string());
    };

    let routing_result = handle.coordinator.route(&request);

    match &routing_result {
        Ok(response) => {
            handle.active_specialist = Some(response.specialist);
            log_routing_decision(
                &response.specialist,
                response.confidence,
                &response.reasoning,
            );

            if response.confidence < 0.7 {
                eprintln!(
                    "[acp::routing] Low confidence routing: specialist={:?}, confidence={:.2}",
                    response.specialist, response.confidence
                );
            }
        }
        Err(err) => {
            eprintln!(
                "[acp::routing] Routing failed: {}. Falling back to default.",
                err
            );
            handle.active_specialist = Some(AgentSpecialist::SqlExpert);

            return Ok(AgentRoutingResponse {
                specialist: AgentSpecialist::SqlExpert,
                confidence: 0.4,
                reasoning: format!("Routing failed ({}). Using default specialist.", err),
            });
        }
    }

    routing_result
}

/// Sends an ACP prompt with agent routing integration.
/// Augments the prompt with execution history context before sending.
/// Maintains backward compatibility with the existing signature.
pub fn send_acp_prompt_with_routing(
    prompt: String,
    database_context: String,
) -> Result<(), String> {
    let mut slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let Some(handle) = slot.as_mut() else {
        return Err("ACP agent is not connected".to_string());
    };

    let routing_request = AgentRoutingRequest {
        query_text: prompt.clone(),
        database_context,
        user_intent: None,
    };

    let routing_response = handle.coordinator.route(&routing_request);

    match &routing_response {
        Ok(response) => {
            handle.active_specialist = Some(response.specialist);
            log_routing_decision(
                &response.specialist,
                response.confidence,
                &response.reasoning,
            );

            let augmented_prompt = augment_prompt_with_context(&prompt, &handle.execution_history);

            if response.confidence >= 0.7
                && let Ok(specialist_response) = handle
                    .coordinator
                    .dispatch(response.specialist, &routing_request)
            {
                eprintln!(
                    "[acp::routing] Dispatched to {:?}: {}",
                    response.specialist,
                    specialist_response.lines().next().unwrap_or("no response")
                );
            }

            drop(slot);
            send_command(AcpCommand::Prompt(augmented_prompt))
        }
        Err(err) => {
            eprintln!("[acp::routing] Routing failed: {}. Using default.", err);
            handle.active_specialist = Some(AgentSpecialist::SqlExpert);
            drop(slot);
            send_command(AcpCommand::Prompt(prompt))
        }
    }
}

/// Logs routing decisions for telemetry and debugging.
fn log_routing_decision(specialist: &AgentSpecialist, confidence: f32, reasoning: &str) {
    eprintln!(
        "[acp::routing] specialist={} confidence={:.2} reasoning={}",
        specialist.variant_name(),
        confidence,
        reasoning
    );
}

/// Augments a prompt with execution history context.
fn augment_prompt_with_context(prompt: &str, execution_history: &[String]) -> String {
    if execution_history.is_empty() {
        return prompt.to_string();
    }

    let mut augmented = String::new();
    augmented.push_str("## Execution History\n");
    augmented.push_str("Recent operations:\n");

    for entry in execution_history.iter().rev().take(5) {
        augmented.push_str(&format!("- {}\n", entry));
    }

    augmented.push('\n');
    augmented.push_str("## Current Request\n");
    augmented.push_str(prompt);

    augmented
}

/// Clears the currently active specialist.
pub fn clear_active_specialist() -> Result<(), String> {
    let mut slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let Some(handle) = slot.as_mut() else {
        return Err("ACP agent is not connected".to_string());
    };

    handle.active_specialist = None;
    handle.coordinator.clear_active();

    Ok(())
}

/// Returns the currently active specialist, if any.
pub fn get_active_specialist() -> Result<Option<AgentSpecialist>, String> {
    let slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let Some(handle) = slot.as_ref() else {
        return Err("ACP agent is not connected".to_string());
    };

    Ok(handle.active_specialist)
}

/// Records an execution entry in the history.
pub fn record_execution(entry: String) -> Result<(), String> {
    let mut slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let Some(handle) = slot.as_mut() else {
        return Err("ACP agent is not connected".to_string());
    };

    handle.execution_history.push(entry);

    if handle.execution_history.len() > 20 {
        handle.execution_history.remove(0);
    }

    Ok(())
}

pub fn cancel_acp_prompt() -> Result<(), String> {
    send_command(AcpCommand::Cancel)
}

pub fn disconnect_acp_agent() -> Result<(), String> {
    let mut slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;

    let handle = slot.take();
    drop(slot);

    if let Some(handle) = handle {
        let request_ids = handle
            .permission_registry
            .pending
            .lock()
            .map_err(|_| "ACP permission registry lock poisoned".to_string())?
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for request_id in request_ids {
            let _ = respond_to_permission_request(&handle.permission_registry, request_id, None);
        }
        for terminal_id in handle.terminal_registry.ids()? {
            let _ = handle.terminal_registry.take(&terminal_id);
        }
        let _ = handle.command_tx.send(AcpCommand::Disconnect);
        let _ = handle.thread.join();
    }

    Ok(())
}

pub fn drain_acp_events() -> Vec<AcpEvent> {
    let mut drained = Vec::new();
    let Ok(mut slot) = runtime_slot().lock() else {
        return drained;
    };
    let Some(handle) = slot.as_mut() else {
        return drained;
    };

    while let Ok(event) = handle.event_rx.try_recv() {
        drained.push(event);
    }

    drained
}

pub fn respond_acp_permission(request_id: u64, option_id: Option<String>) -> Result<(), String> {
    let slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;
    let Some(handle) = slot.as_ref() else {
        return Err("ACP agent is not connected".to_string());
    };

    respond_to_permission_request(&handle.permission_registry, request_id, option_id)
}

fn send_command(command: AcpCommand) -> Result<(), String> {
    let slot = runtime_slot()
        .lock()
        .map_err(|_| "ACP runtime lock poisoned".to_string())?;
    let Some(handle) = slot.as_ref() else {
        return Err("ACP agent is not connected".to_string());
    };

    handle
        .command_tx
        .send(command)
        .map_err(|_| "ACP worker is not available".to_string())
}

async fn run_acp_worker(
    request: AcpLaunchRequest,
    mut command_rx: tokio_mpsc::UnboundedReceiver<AcpCommand>,
    event_tx: mpsc::Sender<AcpEvent>,
    ready_tx: oneshot::Sender<Result<AcpConnectionInfo, String>>,
    permission_registry: PermissionRegistry,
    terminal_registry: TerminalRegistry,
) {
    let command = request.command.trim();
    if command.is_empty() {
        let _ = ready_tx.send(Err("ACP command is empty".to_string()));
        return;
    }

    let args = shlex::split(request.args.trim())
        .ok_or_else(|| "Failed to parse ACP arguments".to_string());
    let args = match args {
        Ok(args) => args,
        Err(err) => {
            let _ = ready_tx.send(Err(err));
            return;
        }
    };

    let cwd = match normalize_cwd(&request.cwd) {
        Ok(cwd) => cwd,
        Err(err) => {
            let _ = ready_tx.send(Err(err));
            return;
        }
    };

    let mut child = match spawn_acp_child(command, &args, &cwd, &request.env).await {
        Ok(child) => child,
        Err(err) => {
            let _ = ready_tx.send(Err(format!("Failed to start ACP agent: {err}")));
            return;
        }
    };

    let Some(stdin) = child.stdin.take() else {
        let _ = ready_tx.send(Err("ACP agent stdin is unavailable".to_string()));
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = ready_tx.send(Err("ACP agent stdout is unavailable".to_string()));
        return;
    };
    let stderr_buffer = Arc::new(Mutex::new(String::new()));
    if let Some(mut stderr) = child.stderr.take() {
        let stderr_buffer = Arc::clone(&stderr_buffer);
        tokio::task::spawn_local(async move {
            let mut chunk = [0_u8; 2048];
            while let Ok(read) = stderr.read(&mut chunk).await {
                if read == 0 {
                    break;
                }
                append_stderr_chunk(&stderr_buffer, &chunk[..read]);
            }
        });
    }

    let (conn, io_task) = acp::ClientSideConnection::new(
        BridgeClient {
            event_tx: event_tx.clone(),
            permission_registry,
            terminal_registry,
            workspace_root: cwd.clone(),
        },
        stdin.compat_write(),
        stdout.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );
    let conn = Rc::new(conn);

    {
        let event_tx = event_tx.clone();
        let stderr_buffer = Arc::clone(&stderr_buffer);
        tokio::task::spawn_local(async move {
            if let Err(err) = io_task.await {
                let _ = event_tx.send(AcpEvent::Error(format_error_with_stderr(
                    format!("ACP I/O error: {err}"),
                    &stderr_buffer,
                )));
            }
        });
    }

    let init_result = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let init_response = conn
            .initialize(
                acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                    .client_capabilities(
                        acp::ClientCapabilities::new()
                            .fs(acp::FileSystemCapabilities::new()
                                .read_text_file(true)
                                .write_text_file(true))
                            .terminal(true),
                    )
                    .client_info(
                        acp::Implementation::new("shovel", env!("CARGO_PKG_VERSION"))
                            .title("Shovel ACP Client"),
                    ),
            )
            .await
            .map_err(|err| {
                format_error_with_stderr(format!("ACP initialize failed: {err}"), &stderr_buffer)
            })?;

        let session_response = conn
            .new_session(acp::NewSessionRequest::new(cwd.clone()))
            .await
            .map_err(|err| {
                format_error_with_stderr(
                    format!("ACP session creation failed: {err}"),
                    &stderr_buffer,
                )
            })?;

        Ok::<_, String>((init_response, session_response))
    })
    .await;

    let (init_response, session_response) = match init_result {
        Ok(Ok(pair)) => pair,
        Ok(Err(err)) => {
            let _ = ready_tx.send(Err(err));
            return;
        }
        Err(_) => {
            let _ = child.kill().await;
            let timeout_msg =
                "ACP agent initialization timed out after 30 seconds. The agent may be unresponsive."
                    .to_string();
            let _ = ready_tx.send(Err(timeout_msg.clone()));
            let _ = event_tx.send(AcpEvent::Error(timeout_msg));
            return;
        }
    };

    let agent_name = init_response
        .agent_info
        .as_ref()
        .and_then(|info| info.title.clone())
        .or_else(|| {
            init_response
                .agent_info
                .as_ref()
                .map(|info| info.name.clone())
        })
        .unwrap_or_else(|| command.to_string());

    let connection = AcpConnectionInfo {
        agent_name: agent_name.clone(),
        session_id: session_response.session_id.to_string(),
        protocol_version: format!("{:?}", init_response.protocol_version),
    };

    if ready_tx.send(Ok(connection.clone())).is_err() {
        return;
    }

    let _ = event_tx.send(AcpEvent::Connected(connection));
    let _ = event_tx.send(AcpEvent::Status(format!("Connected to {agent_name}")));

    let active_session_id = Rc::new(RefCell::new(session_response.session_id.to_string()));

    while let Some(command) = command_rx.recv().await {
        match command {
            AcpCommand::Prompt(prompt) => {
                let conn = Rc::clone(&conn);
                let event_tx = event_tx.clone();
                let active_session_id = Rc::clone(&active_session_id);

                tokio::task::spawn_local(async move {
                    let session_id = active_session_id.borrow().clone();

                    let _ = event_tx.send(AcpEvent::PromptStarted);
                    match conn
                        .prompt(acp::PromptRequest::new(
                            session_id,
                            vec![ContentBlock::from(prompt)],
                        ))
                        .await
                    {
                        Ok(response) => {
                            let _ = event_tx.send(AcpEvent::PromptFinished {
                                stop_reason: format!("{:?}", response.stop_reason),
                            });
                        }
                        Err(err) => {
                            let _ =
                                event_tx.send(AcpEvent::Error(format!("ACP prompt failed: {err}")));
                        }
                    }
                });
            }
            AcpCommand::Cancel => {
                let session_id = active_session_id.borrow().clone();
                if let Err(err) = conn.cancel(acp::CancelNotification::new(session_id)).await {
                    let _ = event_tx.send(AcpEvent::Error(format!("ACP cancel failed: {err}")));
                } else {
                    let _ = event_tx.send(AcpEvent::Status("Cancelling prompt...".to_string()));
                }
            }
            AcpCommand::Disconnect => {
                break;
            }
        }
    }

    drop(child);
    let _ = event_tx.send(AcpEvent::Disconnected);
}

async fn spawn_acp_child(
    command: &str,
    args: &[String],
    cwd: &PathBuf,
    env: &[(String, String)],
) -> Result<Child, std::io::Error> {
    const TEXT_FILE_BUSY_RETRIES: usize = 6;
    const TEXT_FILE_BUSY_DELAY_MS: u64 = 150;

    let mut attempt = 0;
    loop {
        let mut child_command = tokio::process::Command::new(command);
        child_command
            .args(args)
            .envs(env.iter().map(|(key, value)| (key, value)))
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        apply_agent_runtime_environment(&mut child_command, command);
        apply_hidden_process_flags(&mut child_command);

        match child_command.spawn() {
            Ok(child) => return Ok(child),
            Err(err) if is_text_file_busy_error(&err) && attempt < TEXT_FILE_BUSY_RETRIES => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(
                    TEXT_FILE_BUSY_DELAY_MS * attempt as u64,
                ))
                .await;
            }
            Err(err) => return Err(err),
        }
    }
}

fn apply_agent_runtime_environment(command: &mut tokio::process::Command, executable: &str) {
    if !is_opencode_command(executable) {
        return;
    }

    let Some(envs) = opencode_runtime_environment() else {
        return;
    };

    for (key, value) in envs {
        command.env(key, value);
    }
}

fn is_opencode_command(executable: &str) -> bool {
    executable
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|file_name| {
            file_name.eq_ignore_ascii_case("opencode")
                || file_name.eq_ignore_ascii_case("opencode.exe")
        })
}

fn opencode_runtime_environment() -> Option<[(&'static str, PathBuf); 3]> {
    let runtime_root = {
        let base = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("shovel")
            .join("acp")
            .join("runtime")
            .join("opencode");
        if std::fs::create_dir_all(&base).is_ok() {
            base
        } else {
            let fallback = std::env::temp_dir()
                .join("shovel")
                .join("acp")
                .join("runtime")
                .join("opencode");
            std::fs::create_dir_all(&fallback).ok()?;
            fallback
        }
    };

    let data_home = runtime_root.join("xdg-data");
    let state_home = runtime_root.join("xdg-state");
    let cache_home = runtime_root.join("xdg-cache");

    for path in [&data_home, &state_home, &cache_home] {
        if std::fs::create_dir_all(path).is_err() {
            return None;
        }
    }

    Some([
        ("XDG_DATA_HOME", data_home),
        ("XDG_STATE_HOME", state_home),
        ("XDG_CACHE_HOME", cache_home),
    ])
}

fn is_text_file_busy_error(err: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        err.raw_os_error() == Some(26)
    }

    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

fn normalize_cwd(cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    let resolved = if trimmed.is_empty() {
        std::env::current_dir().map_err(|err| format!("Failed to resolve cwd: {err}"))?
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(path))
                .map_err(|err| format!("Failed to resolve cwd: {err}"))?
        }
    };

    normalize_cwd_path(resolved)
}

fn normalize_cwd_path(path: PathBuf) -> Result<PathBuf, String> {
    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.is_dir() {
            return Ok(path);
        }

        if metadata.is_file() {
            return path
                .parent()
                .map(PathBuf::from)
                .ok_or_else(|| format!("ACP cwd has no parent directory: {}", path.display()));
        }
    }

    let mut ancestor = path.parent();
    while let Some(parent) = ancestor {
        if parent.is_dir() {
            return Ok(parent.to_path_buf());
        }
        ancestor = parent.parent();
    }

    Ok(path)
}

fn apply_hidden_process_flags(_command: &mut tokio::process::Command) {
    #[cfg(windows)]
    _command.creation_flags(CREATE_NO_WINDOW);
}

fn append_stderr_chunk(buffer: &Arc<Mutex<String>>, bytes: &[u8]) {
    let chunk = String::from_utf8_lossy(bytes);
    let Ok(mut stderr) = buffer.lock() else {
        return;
    };
    stderr.push_str(&chunk);
    const MAX_STDERR_CHARS: usize = 8192;
    if stderr.len() > MAX_STDERR_CHARS {
        let split_index = stderr.len() - MAX_STDERR_CHARS;
        stderr.drain(..split_index);
    }
}

fn format_error_with_stderr(message: String, stderr_buffer: &Arc<Mutex<String>>) -> String {
    let Ok(stderr) = stderr_buffer.lock() else {
        return message;
    };
    let stderr = stderr.trim();
    if stderr.is_empty() {
        return message;
    }

    const MAX_STDERR_EXCERPT: usize = 600;
    let excerpt = if stderr.len() > MAX_STDERR_EXCERPT {
        &stderr[stderr.len() - MAX_STDERR_EXCERPT..]
    } else {
        stderr
    };
    format!("{message}. Agent stderr: {excerpt}")
}

fn resolve_workspace_path(
    workspace_root: &std::path::Path,
    requested_path: &std::path::Path,
    allow_missing_leaf: bool,
) -> Result<PathBuf, String> {
    if !requested_path.is_absolute() {
        return Err("ACP file path must be absolute".to_string());
    }

    let canonical_root = std::fs::canonicalize(workspace_root)
        .map_err(|err| format!("Failed to resolve ACP workspace root: {err}"))?;

    let resolved = if allow_missing_leaf && !requested_path.exists() {
        resolve_future_workspace_path(&canonical_root, requested_path)?
    } else {
        std::fs::canonicalize(requested_path)
            .map_err(|err| format!("Failed to resolve ACP file path: {err}"))?
    };

    if !resolved.starts_with(&canonical_root) {
        return Err(format!(
            "ACP file access outside workspace is not allowed: {}",
            resolved.display()
        ));
    }

    Ok(resolved)
}

fn resolve_future_workspace_path(
    canonical_root: &std::path::Path,
    requested_path: &std::path::Path,
) -> Result<PathBuf, String> {
    let mut existing = requested_path;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| "ACP file path has no existing ancestor".to_string())?;
    }

    let canonical_existing = std::fs::canonicalize(existing)
        .map_err(|err| format!("Failed to resolve ACP path ancestor: {err}"))?;

    if !canonical_existing.starts_with(canonical_root) {
        return Err(format!(
            "ACP file access outside workspace is not allowed: {}",
            requested_path.display()
        ));
    }

    Ok(requested_path.to_path_buf())
}

fn apply_read_window(content: String, line: Option<u32>, limit: Option<u32>) -> String {
    let start = line.unwrap_or(1).max(1) as usize - 1;
    let limit = limit.map(|value| value as usize);
    let lines = content.lines().collect::<Vec<_>>();
    let selected = lines
        .into_iter()
        .skip(start)
        .take(limit.unwrap_or(usize::MAX))
        .collect::<Vec<_>>();

    if selected.is_empty() {
        String::new()
    } else {
        selected.join("\n")
    }
}

async fn read_terminal_stream<R>(
    terminal: Arc<TerminalState>,
    mut reader: R,
    event_tx: mpsc::Sender<AcpEvent>,
    terminal_id: String,
    stream_label: &'static str,
) where
    R: tokio::io::AsyncRead + Unpin + 'static,
{
    let mut buffer = vec![0_u8; 4096];

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
                if let Err(err) = append_terminal_output(&terminal, &chunk) {
                    let _ = event_tx.send(AcpEvent::Error(err));
                    break;
                }
            }
            Err(err) => {
                let _ = event_tx.send(AcpEvent::Error(format!(
                    "ACP terminal {terminal_id} {stream_label} read failed: {err}"
                )));
                break;
            }
        }
    }

    if let Err(err) = update_terminal_exit_status(&terminal).await {
        let _ = event_tx.send(AcpEvent::Error(format!(
            "ACP terminal {terminal_id} exit status update failed: {err}"
        )));
    }
}

fn append_terminal_output(terminal: &Arc<TerminalState>, chunk: &str) -> Result<(), String> {
    let mut output = terminal
        .output
        .lock()
        .map_err(|_| "ACP terminal output lock poisoned".to_string())?;
    output.push_str(chunk);

    if let Some(limit) = terminal.output_limit
        && output.len() > limit
    {
        let trim_target = output.len() - limit;
        let trim_at = output
            .char_indices()
            .map(|(idx, _)| idx)
            .find(|idx| *idx >= trim_target)
            .unwrap_or(output.len());
        output.drain(..trim_at);
        *terminal
            .truncated
            .lock()
            .map_err(|_| "ACP terminal truncation lock poisoned".to_string())? = true;
    }

    Ok(())
}

async fn update_terminal_exit_status(terminal: &Arc<TerminalState>) -> Result<(), std::io::Error> {
    if terminal
        .exit_status
        .lock()
        .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))?
        .is_some()
    {
        return Ok(());
    }

    let status = {
        let mut child = terminal.child.lock().await;
        child.try_wait()?
    };

    if let Some(status) = status {
        *terminal
            .exit_status
            .lock()
            .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))? =
            Some(terminal_exit_status(status));
    }

    Ok(())
}

async fn wait_for_terminal_exit_status(
    terminal: &Arc<TerminalState>,
) -> Result<acp::TerminalExitStatus, std::io::Error> {
    if let Some(status) = terminal
        .exit_status
        .lock()
        .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))?
        .clone()
    {
        return Ok(status);
    }

    let status = {
        let mut child = terminal.child.lock().await;
        child.wait().await?
    };
    let status = terminal_exit_status(status);
    *terminal
        .exit_status
        .lock()
        .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))? =
        Some(status.clone());

    Ok(status)
}

async fn terminate_terminal(terminal: &Arc<TerminalState>) -> Result<(), std::io::Error> {
    if terminal
        .exit_status
        .lock()
        .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))?
        .is_some()
    {
        return Ok(());
    }

    let status = {
        let mut child = terminal.child.lock().await;
        match child.try_wait()? {
            Some(status) => status,
            None => {
                child.kill().await?;
                child.wait().await?
            }
        }
    };

    *terminal
        .exit_status
        .lock()
        .map_err(|_| std::io::Error::other("ACP terminal exit status lock poisoned"))? =
        Some(terminal_exit_status(status));

    Ok(())
}

fn terminal_exit_status(status: std::process::ExitStatus) -> acp::TerminalExitStatus {
    let exit_code = status.code().map(|code| code as u32);
    #[cfg(unix)]
    let signal = status.signal().map(|signal| signal.to_string());
    #[cfg(not(unix))]
    let signal = None::<String>;

    acp::TerminalExitStatus::new()
        .exit_code(exit_code)
        .signal(signal)
}

fn send_chunk(event_tx: &mpsc::Sender<AcpEvent>, kind: AcpMessageKind, chunk: ContentChunk) {
    let text = content_to_text(chunk.content);
    if text.is_empty() {
        return;
    }

    let _ = event_tx.send(AcpEvent::Message { kind, text });
}

fn content_to_text(content: ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text,
        ContentBlock::Image(_) => "<image>".to_string(),
        ContentBlock::Audio(_) => "<audio>".to_string(),
        ContentBlock::ResourceLink(link) => link.uri,
        ContentBlock::Resource(_) => "<resource>".to_string(),
        _ => "<content>".to_string(),
    }
}

fn respond_to_permission_request(
    registry: &PermissionRegistry,
    request_id: u64,
    option_id: Option<String>,
) -> Result<(), String> {
    let sender = registry
        .take(request_id)?
        .ok_or_else(|| "Permission request no longer exists".to_string())?;

    let response = match option_id {
        Some(option_id) => RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(option_id),
        )),
        None => RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
    };

    sender
        .send(response)
        .map_err(|_| "Permission request receiver dropped".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        acp, format_error_with_stderr, is_opencode_command, is_text_file_busy_error,
        normalize_cwd_path, opencode_runtime_environment,
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn normalize_cwd_path_keeps_existing_directory() {
        let dir = std::env::temp_dir();
        assert_eq!(normalize_cwd_path(dir.clone()).unwrap(), dir);
    }

    #[test]
    fn normalize_cwd_path_uses_parent_for_existing_file() {
        let temp_root =
            std::env::temp_dir().join(format!("shovel-acp-runtime-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp_root).unwrap();
        let file = temp_root.join("workspace.db");
        std::fs::write(&file, b"test").unwrap();

        assert_eq!(normalize_cwd_path(file).unwrap(), temp_root);
    }

    #[test]
    fn normalize_cwd_path_uses_existing_parent_for_missing_file_like_path() {
        let dir = std::env::temp_dir();
        let path = dir.join("missing").join("project.db");

        assert_eq!(normalize_cwd_path(path).unwrap(), dir);
    }

    #[test]
    fn initialize_request_uses_numeric_protocol_version() {
        let request = acp::InitializeRequest::new(acp::ProtocolVersion::LATEST)
            .client_capabilities(acp::ClientCapabilities::new().terminal(true))
            .client_info(acp::Implementation::new("shovel", "test"));

        let payload = serde_json::to_value(&request).unwrap();

        assert_eq!(payload["protocolVersion"], serde_json::json!(1));
    }

    #[test]
    fn format_error_with_stderr_appends_excerpt() {
        let stderr = Arc::new(Mutex::new("line 1\nline 2".to_string()));
        let message = format_error_with_stderr("ACP initialize failed".to_string(), &stderr);

        assert!(message.contains("ACP initialize failed"));
        assert!(message.contains("Agent stderr: line 1\nline 2"));
    }

    #[cfg(unix)]
    #[test]
    fn detects_text_file_busy_os_error() {
        let err = std::io::Error::from_raw_os_error(26);
        assert!(is_text_file_busy_error(&err));
    }

    #[test]
    fn detects_opencode_command_by_file_name() {
        assert!(is_opencode_command("opencode"));
        assert!(is_opencode_command("/tmp/tools/opencode"));
        assert!(is_opencode_command(r"C:\tools\opencode.exe"));
        assert!(!is_opencode_command("codex-acp"));
    }

    #[test]
    fn opencode_runtime_environment_isolated_under_shovel_storage() {
        let envs = opencode_runtime_environment().expect("opencode env should be available");

        for (key, value) in envs {
            assert!(matches!(
                key,
                "XDG_DATA_HOME" | "XDG_STATE_HOME" | "XDG_CACHE_HOME"
            ));
            let value = value.to_string_lossy().to_string();
            assert!(value.contains("shovel/acp/runtime/opencode"));
        }
    }

    #[test]
    fn send_acp_prompt_returns_error_without_connected_agent() {
        let result = super::send_acp_prompt("select 1".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ACP agent is not connected"));
    }

    #[test]
    fn disconnect_acp_agent_returns_error_without_connected_agent() {
        let result = super::disconnect_acp_agent();
        assert!(result.is_ok());
    }

    #[test]
    fn cancel_acp_prompt_returns_error_without_connected_agent() {
        let result = super::cancel_acp_prompt();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ACP agent is not connected"));
    }

    #[test]
    fn respond_acp_permission_returns_error_without_connected_agent() {
        let result = super::respond_acp_permission(0, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ACP agent is not connected"));
    }
}
