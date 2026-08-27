//! In-process OpenAI-compatible and Ollama native chat streaming.

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use futures_util::{
    StreamExt,
    future::Either,
    stream::{self, Stream},
};
use models::normalize_native_chat_url;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

const NATIVE_CHAT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const NATIVE_CHAT_TIMEOUT: Duration = Duration::from_secs(120);
const NATIVE_CHAT_CANCEL_POLL: Duration = Duration::from_millis(50);
const COMPLETION_MAX_TOKENS: u32 = 100;
const COMPLETION_TEMPERATURE: f64 = 0.2;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(15);

static NATIVE_CHAT_CANCEL: AtomicBool = AtomicBool::new(false);

/// Request cancellation of an in-flight native chat stream.
pub fn request_native_chat_cancel() {
    NATIVE_CHAT_CANCEL.store(true, Ordering::SeqCst);
}

/// Clear the native chat cancel flag (call before starting a new prompt).
pub fn clear_native_chat_cancel() {
    NATIVE_CHAT_CANCEL.store(false, Ordering::SeqCst);
}

/// Whether native chat cancel has been requested.
pub fn native_chat_cancel_requested() -> bool {
    NATIVE_CHAT_CANCEL.load(Ordering::SeqCst)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeChatRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<NativeChatMessage>,
    pub provider_slug: String,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NativeChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeChatEvent {
    Delta(String),
    Thought(String),
    Finished,
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionToken {
    Text(String),
    Done,
    Error(String),
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChatMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatMessage {
    #[serde(default)]
    content: String,
}

/// Build the OpenAI-compatible chat completions JSON body.
pub fn openai_request_body(req: &NativeChatRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "messages": req.messages,
        "stream": true,
    });

    if req.provider_slug == "deepseek" {
        let obj = body.as_object_mut().expect("request body is object");
        if req.thinking_enabled {
            obj.insert("thinking".into(), json!({ "type": "enabled" }));
        }
        obj.insert(
            "reasoning_effort".into(),
            Value::String(normalize_reasoning_effort_value(&req.reasoning_effort).to_string()),
        );
    }

    body
}

fn ollama_request_body(req: &NativeChatRequest) -> Value {
    json!({
        "model": req.model,
        "messages": req.messages,
        "stream": true,
    })
}

pub fn completion_request_body(req: &NativeChatRequest) -> Value {
    let stop = json!(["\n\n", "```"]);
    if req.provider_slug == "ollama" {
        json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "options": {
                "num_predict": COMPLETION_MAX_TOKENS,
                "temperature": COMPLETION_TEMPERATURE,
                "stop": stop,
            }
        })
    } else {
        json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "max_tokens": COMPLETION_MAX_TOKENS,
            "temperature": COMPLETION_TEMPERATURE,
            "stop": stop,
        })
    }
}

pub fn completion_token_from_event(event: NativeChatEvent) -> Option<CompletionToken> {
    match event {
        NativeChatEvent::Delta(text) => Some(CompletionToken::Text(text)),
        NativeChatEvent::Thought(_) => None,
        NativeChatEvent::Finished => None,
        NativeChatEvent::Error(text) => Some(CompletionToken::Error(text)),
    }
}

fn normalize_reasoning_effort_value(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn chat_url(req: &NativeChatRequest) -> String {
    let base = req.base_url.as_str();
    let normalized = normalize_native_chat_url(base, base);

    if req.provider_slug == "ollama" {
        if normalized.ends_with("/api") {
            format!("{normalized}/chat")
        } else {
            format!("{normalized}/api/chat")
        }
    } else if base.contains("chat/completions") || normalized.contains("chat/completions") {
        normalized
    } else if normalized.ends_with("/v1") {
        format!("{normalized}/chat/completions")
    } else {
        format!("{normalized}/v1/chat/completions")
    }
}

/// Parse an OpenAI-compatible SSE body into chat events (no network).
pub fn parse_openai_sse(body: &str) -> Vec<NativeChatEvent> {
    let mut events = Vec::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            events.push(NativeChatEvent::Finished);
            continue;
        }

        let Ok(chunk) = serde_json::from_str::<OpenAiChatChunk>(data) else {
            continue;
        };

        if let Some(error) = chunk.error {
            events.push(NativeChatEvent::Error(error.to_string()));
            continue;
        }

        if let Some(choice) = chunk.choices.first() {
            if let Some(reasoning) = choice.delta.reasoning_content.as_ref()
                && !reasoning.is_empty()
            {
                events.push(NativeChatEvent::Thought(reasoning.clone()));
            }
            if let Some(content) = choice.delta.content.as_ref()
                && !content.is_empty()
            {
                events.push(NativeChatEvent::Delta(content.clone()));
            }
        }
    }
    events
}

fn parse_ollama_ndjson_line(line: &str) -> Vec<NativeChatEvent> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }

    let Ok(chunk) = serde_json::from_str::<OllamaChatChunk>(line) else {
        return Vec::new();
    };

    let mut events = Vec::new();
    if let Some(error) = chunk.error {
        events.push(NativeChatEvent::Error(error));
        return events;
    }
    if let Some(message) = chunk.message
        && !message.content.is_empty()
    {
        events.push(NativeChatEvent::Delta(message.content));
    }
    if chunk.done {
        events.push(NativeChatEvent::Finished);
    }
    events
}

fn auth_headers(api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let trimmed = api_key.trim();
    if !trimmed.is_empty() {
        let value = HeaderValue::from_str(&format!("Bearer {trimmed}"))
            .map_err(|err| format!("Invalid API key header: {err}"))?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn is_terminal_event(event: &NativeChatEvent) -> bool {
    matches!(event, NativeChatEvent::Finished | NativeChatEvent::Error(_))
}

/// Split complete newline-delimited frames out of `buffer` and parse them.
fn take_complete_line_events(buffer: &mut String, is_ollama: bool) -> Vec<NativeChatEvent> {
    let mut events = Vec::new();
    while let Some(idx) = buffer.find('\n') {
        let mut line = buffer[..idx].to_string();
        buffer.drain(..=idx);
        if line.ends_with('\r') {
            line.pop();
        }
        if is_ollama {
            events.extend(parse_ollama_ndjson_line(&line));
        } else {
            events.extend(parse_openai_sse(&line));
        }
        if events.iter().any(is_terminal_event) {
            break;
        }
    }
    events
}

fn flush_remaining_buffer(buffer: &mut String, is_ollama: bool) -> Vec<NativeChatEvent> {
    if buffer.trim().is_empty() {
        return Vec::new();
    }
    let rest = std::mem::take(buffer);
    if is_ollama {
        parse_ollama_ndjson_line(&rest)
    } else {
        parse_openai_sse(&rest)
    }
}

async fn wait_for_native_cancel() {
    while !native_chat_cancel_requested() {
        tokio::time::sleep(NATIVE_CHAT_CANCEL_POLL).await;
    }
}

struct NativeBodyState<S> {
    byte_stream: S,
    buffer: String,
    is_ollama: bool,
    pending: VecDeque<NativeChatEvent>,
    finished: bool,
}

/// Yield parsed chat events as HTTP body chunks arrive. Dropping the stream
/// aborts the request. Cancel is polled between chunks so Cancel does not wait
/// for the full body.
fn live_native_event_stream<S, B, E>(
    byte_stream: S,
    is_ollama: bool,
) -> impl Stream<Item = NativeChatEvent>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let state = NativeBodyState {
        byte_stream,
        buffer: String::new(),
        is_ollama,
        pending: VecDeque::new(),
        finished: false,
    };

    stream::unfold(state, |mut state| async move {
        if let Some(event) = state.pending.pop_front() {
            return Some((event, state));
        }
        if state.finished {
            return None;
        }

        loop {
            if native_chat_cancel_requested() {
                state.finished = true;
                return Some((NativeChatEvent::Error("Cancelled".into()), state));
            }

            tokio::select! {
                biased;
                _ = wait_for_native_cancel() => {
                    state.finished = true;
                    return Some((NativeChatEvent::Error("Cancelled".into()), state));
                }
                chunk = state.byte_stream.next() => {
                    match chunk {
                        None => {
                            let mut events =
                                flush_remaining_buffer(&mut state.buffer, state.is_ollama);
                            if !events.iter().any(is_terminal_event) {
                                events.push(NativeChatEvent::Finished);
                            }
                            state.pending.extend(events);
                            state.finished = true;
                            return state.pending.pop_front().map(|event| (event, state));
                        }
                        Some(Err(err)) => {
                            state.finished = true;
                            return Some((
                                NativeChatEvent::Error(format!(
                                    "Native chat stream failed: {err}"
                                )),
                                state,
                            ));
                        }
                        Some(Ok(bytes)) => {
                            state
                                .buffer
                                .push_str(&String::from_utf8_lossy(bytes.as_ref()));
                            let events =
                                take_complete_line_events(&mut state.buffer, state.is_ollama);
                            if events.is_empty() {
                                continue;
                            }
                            for event in events {
                                let terminal = is_terminal_event(&event);
                                state.pending.push_back(event);
                                if terminal {
                                    state.finished = true;
                                    break;
                                }
                            }
                            return state.pending.pop_front().map(|event| (event, state));
                        }
                    }
                }
            }
        }
    })
}

/// POST to the provider chat endpoint and yield parsed events.
pub async fn stream_native_chat(
    req: NativeChatRequest,
) -> Result<impl Stream<Item = NativeChatEvent>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(NATIVE_CHAT_CONNECT_TIMEOUT)
        .timeout(NATIVE_CHAT_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let url = chat_url(&req);
    let is_ollama = req.provider_slug == "ollama";
    let body = if is_ollama {
        ollama_request_body(&req)
    } else {
        openai_request_body(&req)
    };

    if native_chat_cancel_requested() {
        return Ok(Either::Left(stream::iter(vec![NativeChatEvent::Error(
            "Cancelled".into(),
        )])));
    }

    let response = client
        .post(&url)
        .headers(auth_headers(&req.api_key)?)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("Native chat request failed: {err}"))?;

    if native_chat_cancel_requested() {
        drop(response);
        return Ok(Either::Left(stream::iter(vec![NativeChatEvent::Error(
            "Cancelled".into(),
        )])));
    }

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Ok(Either::Left(stream::iter(vec![NativeChatEvent::Error(
            "Auth failed".into(),
        )])));
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Ok(Either::Left(stream::iter(vec![NativeChatEvent::Error(
            format!("{status}: {}", body.trim()),
        )])));
    }

    let byte_stream = response
        .bytes_stream()
        .map(|chunk| chunk.map(|b| b.to_vec()))
        .boxed();
    Ok(Either::Right(live_native_event_stream(
        byte_stream,
        is_ollama,
    )))
}

/// Yield parsed chat events as HTTP body chunks arrive. Dropping the stream
/// aborts the request. Does not observe native chat cancel.
fn live_completion_event_stream<S, B, E>(
    byte_stream: S,
    is_ollama: bool,
) -> impl Stream<Item = NativeChatEvent>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let state = NativeBodyState {
        byte_stream,
        buffer: String::new(),
        is_ollama,
        pending: VecDeque::new(),
        finished: false,
    };

    stream::unfold(state, |mut state| async move {
        if let Some(event) = state.pending.pop_front() {
            return Some((event, state));
        }
        if state.finished {
            return None;
        }

        loop {
            match state.byte_stream.next().await {
                None => {
                    let mut events = flush_remaining_buffer(&mut state.buffer, state.is_ollama);
                    if !events.iter().any(is_terminal_event) {
                        events.push(NativeChatEvent::Finished);
                    }
                    state.pending.extend(events);
                    state.finished = true;
                    return state.pending.pop_front().map(|event| (event, state));
                }
                Some(Err(err)) => {
                    state.finished = true;
                    return Some((
                        NativeChatEvent::Error(format!("Native chat stream failed: {err}")),
                        state,
                    ));
                }
                Some(Ok(bytes)) => {
                    state
                        .buffer
                        .push_str(&String::from_utf8_lossy(bytes.as_ref()));
                    let events = take_complete_line_events(&mut state.buffer, state.is_ollama);
                    if events.is_empty() {
                        continue;
                    }
                    for event in events {
                        let terminal = is_terminal_event(&event);
                        state.pending.push_back(event);
                        if terminal {
                            state.finished = true;
                            break;
                        }
                    }
                    return state.pending.pop_front().map(|event| (event, state));
                }
            }
        }
    })
}

/// POST to the provider chat endpoint and yield completion tokens.
/// Does not observe native chat cancel; dropping the receiver aborts the task.
pub fn stream_native_completion(
    req: NativeChatRequest,
) -> tokio::sync::mpsc::UnboundedReceiver<CompletionToken> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let stream = match open_native_completion_stream(req).await {
            Ok(stream) => stream,
            Err(err) => {
                let _ = tx.send(CompletionToken::Error(err));
                return;
            }
        };
        futures_util::pin_mut!(stream);
        let mut sent_error_or_done = false;
        while let Some(event) = stream.next().await {
            if let Some(token) = completion_token_from_event(event) {
                if matches!(token, CompletionToken::Error(_) | CompletionToken::Done) {
                    sent_error_or_done = true;
                }
                if tx.send(token).is_err() {
                    return;
                }
                if sent_error_or_done {
                    return;
                }
            }
        }
        if !sent_error_or_done {
            let _ = tx.send(CompletionToken::Done);
        }
    });
    rx
}

async fn open_native_completion_stream(
    req: NativeChatRequest,
) -> Result<impl Stream<Item = NativeChatEvent>, String> {
    let client = reqwest::Client::builder()
        .timeout(COMPLETION_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let url = chat_url(&req);
    let is_ollama = req.provider_slug == "ollama";
    let body = completion_request_body(&req);

    let response = client
        .post(&url)
        .headers(auth_headers(&req.api_key)?)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("Native chat request failed: {err}"))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("Auth failed".into());
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{status}: {}", body.trim()));
    }

    let byte_stream = response
        .bytes_stream()
        .map(|chunk| chunk.map(|b| b.to_vec()))
        .boxed();
    Ok(live_completion_event_stream(byte_stream, is_ollama))
}

/// Serializes tests that touch the process-wide chat-cancel flag: it is
/// global state, so parallel tests would otherwise arm or clear each
/// other's flag mid-run and make the streaming test flaky.
#[cfg(test)]
pub(crate) fn test_cancel_flag_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &LOCK
}

#[cfg(test)]
mod tests {
    use super::{
        CompletionToken,
        NativeChatEvent,
        NativeChatMessage,
        NativeChatRequest,
        chat_url,
        clear_native_chat_cancel,
        completion_request_body,
        completion_token_from_event,
        live_native_event_stream,
        openai_request_body,
        parse_ollama_ndjson_line,
        parse_openai_sse,
        request_native_chat_cancel,
        stream,
        take_complete_line_events,
    };

    /// Tests here share the module-level `test_cancel_flag_lock`.
    fn cancel_flag_lock() -> &'static tokio::sync::Mutex<()> {
        super::test_cancel_flag_lock()
    }

    #[test]
    fn parse_openai_sse_emits_deltas_and_finished() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let events = parse_openai_sse(body);
        assert_eq!(
            events,
            vec![
                NativeChatEvent::Delta("Hel".into()),
                NativeChatEvent::Delta("lo".into()),
                NativeChatEvent::Finished,
            ]
        );
    }

    #[test]
    fn openai_request_json_uses_active_model() {
        let req = NativeChatRequest {
            base_url: "https://api.openai.com".into(),
            api_key: "sk".into(),
            model: "gpt-4o-mini".into(),
            messages: vec![NativeChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            provider_slug: "openai".into(),
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        let v = openai_request_body(&req);
        assert_eq!(v["model"], "gpt-4o-mini");
        assert_eq!(v["stream"], true);
    }

    #[test]
    fn parse_openai_sse_emits_thought_and_error() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"error\":{\"message\":\"nope\"}}\n\n",
        );
        let events = parse_openai_sse(body);
        assert_eq!(
            events,
            vec![
                NativeChatEvent::Thought("hmm".into()),
                NativeChatEvent::Error(r#"{"message":"nope"}"#.into()),
            ]
        );
    }

    #[test]
    fn deepseek_request_includes_thinking_fields() {
        let req = NativeChatRequest {
            base_url: "https://api.deepseek.com".into(),
            api_key: "sk".into(),
            model: "deepseek-chat".into(),
            messages: vec![NativeChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            provider_slug: "deepseek".into(),
            thinking_enabled: true,
            reasoning_effort: "high".into(),
        };
        let v = openai_request_body(&req);
        assert_eq!(v["thinking"]["type"], "enabled");
        assert_eq!(v["reasoning_effort"], "high");
    }

    #[test]
    fn chat_url_openai_and_ollama() {
        let openai = NativeChatRequest {
            base_url: "https://api.openai.com/".into(),
            api_key: String::new(),
            model: "m".into(),
            messages: vec![],
            provider_slug: "openai".into(),
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        assert_eq!(
            chat_url(&openai),
            "https://api.openai.com/v1/chat/completions"
        );

        let full = NativeChatRequest {
            base_url: "https://example.com/v1/chat/completions".into(),
            ..openai.clone()
        };
        assert_eq!(chat_url(&full), "https://example.com/v1/chat/completions");

        // Bases that already end with /v1 must not become .../v1/v1/chat/completions.
        let minimax = NativeChatRequest {
            base_url: "https://api.minimax.chat/v1".into(),
            ..openai.clone()
        };
        assert_eq!(
            chat_url(&minimax),
            "https://api.minimax.chat/v1/chat/completions"
        );

        let ollama = NativeChatRequest {
            base_url: "http://localhost:11434".into(),
            provider_slug: "ollama".into(),
            ..openai.clone()
        };
        assert_eq!(chat_url(&ollama), "http://localhost:11434/api/chat");

        let ollama_api = NativeChatRequest {
            base_url: "http://localhost:11434/api".into(),
            provider_slug: "ollama".into(),
            ..openai
        };
        assert_eq!(chat_url(&ollama_api), "http://localhost:11434/api/chat");
    }

    #[test]
    fn parse_ollama_ndjson_delta_and_done() {
        let events = parse_ollama_ndjson_line(
            r#"{"message":{"role":"assistant","content":"hi"},"done":false}"#,
        );
        assert_eq!(events, vec![NativeChatEvent::Delta("hi".into())]);

        let done = parse_ollama_ndjson_line(r#"{"message":{"content":""},"done":true}"#);
        assert_eq!(done, vec![NativeChatEvent::Finished]);
    }

    #[test]
    fn take_complete_line_events_holds_partial_frames() {
        let mut buffer = String::from("data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n");
        buffer.push_str("data: {\"choices\":[{\"delta\":{\"content\":\"lo");
        let events = take_complete_line_events(&mut buffer, false);
        assert_eq!(events, vec![NativeChatEvent::Delta("Hel".into())]);
        assert!(buffer.contains("lo"));
        buffer.push_str("\"}}]}\n");
        let events = take_complete_line_events(&mut buffer, false);
        assert_eq!(events, vec![NativeChatEvent::Delta("lo".into())]);
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn live_stream_emits_deltas_then_stops_on_cancel() {
        use futures_util::StreamExt as _;

        let _guard = cancel_flag_lock().lock().await;
        clear_native_chat_cancel();
        let chunks: Vec<Result<Vec<u8>, String>> = vec![
            Ok(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n".to_vec()),
            Ok(b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n".to_vec()),
            Ok(b"data: [DONE]\n".to_vec()),
        ];
        let stream = live_native_event_stream(stream::iter(chunks), false);
        futures_util::pin_mut!(stream);
        assert_eq!(
            stream.next().await,
            Some(NativeChatEvent::Delta("Hel".into()))
        );
        request_native_chat_cancel();
        assert_eq!(
            stream.next().await,
            Some(NativeChatEvent::Error("Cancelled".into()))
        );
        assert_eq!(stream.next().await, None);
        clear_native_chat_cancel();
    }

    #[test]
    fn completion_request_body_sets_max_tokens_temperature_and_stop() {
        let req = NativeChatRequest {
            base_url: "https://api.openai.com".into(),
            api_key: "sk".into(),
            model: "gpt-4o-mini".into(),
            messages: vec![NativeChatMessage {
                role: "user".into(),
                content: "select ".into(),
            }],
            provider_slug: "openai".into(),
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        let body = completion_request_body(&req);
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["stop"][0], "\n\n");
        assert_eq!(body["stop"][1], "```");
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn completion_ollama_body_uses_options_not_chat_cancel() {
        let _guard = cancel_flag_lock().blocking_lock();
        request_native_chat_cancel();
        let req = NativeChatRequest {
            base_url: "http://localhost:11434".into(),
            api_key: String::new(),
            model: "qwen2.5-coder".into(),
            messages: vec![],
            provider_slug: "ollama".into(),
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        let body = completion_request_body(&req);
        clear_native_chat_cancel();
        assert_eq!(body["options"]["num_predict"], 100);
        assert_eq!(body["options"]["temperature"], 0.2);
        assert_eq!(body["options"]["stop"][0], "\n\n");
    }

    #[test]
    fn map_native_event_to_completion_ignores_thoughts() {
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Thought("hmm".into())),
            None
        );
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Delta("FROM".into())),
            Some(CompletionToken::Text("FROM".into()))
        );
        assert_eq!(completion_token_from_event(NativeChatEvent::Finished), None);
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Error("nope".into())),
            Some(CompletionToken::Error("nope".into()))
        );
    }
}
