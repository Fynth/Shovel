//! In-process OpenAI-compatible and Ollama native chat streaming.

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[cfg(test)]
use std::sync::Mutex;

use futures_util::{
    StreamExt,
    future::Either,
    stream::{self, Stream},
};
use models::AiBackendId;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::backends::{CompletionToken, backend};

const NATIVE_CHAT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const NATIVE_CHAT_TIMEOUT: Duration = Duration::from_secs(120);
const NATIVE_CHAT_CANCEL_POLL: Duration = Duration::from_millis(50);

static NATIVE_CHAT_CANCEL: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static NATIVE_CHAT_CANCEL_TEST_LOCK: Mutex<()> = Mutex::new(());

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

#[cfg(test)]
pub(crate) fn native_chat_cancel_test_lock() -> std::sync::MutexGuard<'static, ()> {
    NATIVE_CHAT_CANCEL_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeChatRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<NativeChatMessage>,
    pub backend: AiBackendId,
    pub supports_thinking: bool,
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

pub(crate) fn auth_headers(api_key: &str) -> Result<HeaderMap, String> {
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
            events.extend(backend(AiBackendId::Ollama).parse_chat(&line));
        } else {
            events.extend(backend(AiBackendId::OpenAiCompat).parse_chat(&line));
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
        backend(AiBackendId::Ollama).parse_chat(&rest)
    } else {
        backend(AiBackendId::OpenAiCompat).parse_chat(&rest)
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

    let url = backend(req.backend).chat_url(&req.base_url)?;
    let is_ollama = req.backend == AiBackendId::Ollama;
    let body = backend(req.backend).chat_body(&req)?;

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

const COMPLETION_MAX_TOKENS: u32 = 100;
const COMPLETION_TEMPERATURE: f64 = 0.2;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(15);

/// Build the OpenAI-compatible chat-completions body used for SQL ghost
/// text: small, low-temperature, stopped at a blank line or code fence.
pub fn completion_request_body(req: &NativeChatRequest) -> Value {
    let stop = json!(["\n\n", "```"]);
    if req.backend == AiBackendId::Ollama {
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

/// Map a chat stream event to a completion token. Thoughts and the
/// terminal `Finished` marker are ignored: ghost text must not receive
/// reasoning output as code.
pub fn completion_token_from_event(event: NativeChatEvent) -> Option<CompletionToken> {
    match event {
        NativeChatEvent::Delta(text) => Some(CompletionToken::Text(text)),
        NativeChatEvent::Thought(_) => None,
        NativeChatEvent::Finished => None,
        NativeChatEvent::Error(text) => Some(CompletionToken::Error(text)),
    }
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
                let _ = tx.send(CompletionToken::Done);
                return;
            }
        };
        futures_util::pin_mut!(stream);
        while let Some(event) = stream.next().await {
            if let Some(token) = completion_token_from_event(event) {
                let terminal = matches!(token, CompletionToken::Error(_) | CompletionToken::Done);
                if tx.send(token).is_err() || terminal {
                    return;
                }
            }
        }
        let _ = tx.send(CompletionToken::Done);
    });
    rx
}

async fn open_native_completion_stream(
    req: NativeChatRequest,
) -> Result<impl Stream<Item = NativeChatEvent>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(COMPLETION_TIMEOUT)
        .timeout(COMPLETION_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let url = backend(req.backend).chat_url(&req.base_url)?;
    let is_ollama = req.backend == AiBackendId::Ollama;
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
    Ok(live_native_event_stream(byte_stream, is_ollama))
}

#[cfg(test)]
mod tests {
    use super::{
        NativeChatEvent,
        NativeChatMessage,
        NativeChatRequest,
        clear_native_chat_cancel,
        completion_request_body,
        completion_token_from_event,
        live_native_event_stream,
        native_chat_cancel_test_lock,
        request_native_chat_cancel,
        stream,
        take_complete_line_events,
    };
    use crate::backends::{CompletionToken, backend};
    use models::AiBackendId;

    #[test]
    fn parse_openai_sse_emits_deltas_and_finished() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let events = backend(AiBackendId::OpenAiCompat).parse_chat(body);
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
            backend: AiBackendId::OpenAiCompat,
            supports_thinking: false,
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        let v = backend(req.backend).chat_body(&req).unwrap();
        assert_eq!(v["model"], "gpt-4o-mini");
        assert_eq!(v["stream"], true);
    }

    #[test]
    fn parse_openai_sse_emits_thought_and_error() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"error\":{\"message\":\"nope\"}}\n\n",
        );
        let events = backend(AiBackendId::OpenAiCompat).parse_chat(body);
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
            backend: AiBackendId::OpenAiCompat,
            supports_thinking: true,
            thinking_enabled: true,
            reasoning_effort: "high".into(),
        };
        let v = backend(req.backend).chat_body(&req).unwrap();
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
            backend: AiBackendId::OpenAiCompat,
            supports_thinking: false,
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        };
        assert_eq!(
            backend(openai.backend).chat_url(&openai.base_url).unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );

        let full = NativeChatRequest {
            base_url: "https://example.com/v1/chat/completions".into(),
            ..openai.clone()
        };
        assert_eq!(
            backend(full.backend).chat_url(&full.base_url).unwrap(),
            "https://example.com/v1/chat/completions"
        );

        // Bases that already end with /v1 must not become .../v1/v1/chat/completions.
        let minimax = NativeChatRequest {
            base_url: "https://api.minimax.chat/v1".into(),
            ..openai.clone()
        };
        assert_eq!(
            backend(minimax.backend)
                .chat_url(&minimax.base_url)
                .unwrap(),
            "https://api.minimax.chat/v1/chat/completions"
        );

        let ollama = NativeChatRequest {
            base_url: "http://localhost:11434".into(),
            backend: AiBackendId::Ollama,
            ..openai.clone()
        };
        assert_eq!(
            backend(ollama.backend).chat_url(&ollama.base_url).unwrap(),
            "http://localhost:11434/api/chat"
        );

        let ollama_api = NativeChatRequest {
            base_url: "http://localhost:11434/api".into(),
            backend: AiBackendId::Ollama,
            ..openai
        };
        assert_eq!(
            backend(ollama_api.backend)
                .chat_url(&ollama_api.base_url)
                .unwrap(),
            "http://localhost:11434/api/chat"
        );
    }

    #[test]
    fn parse_ollama_ndjson_delta_and_done() {
        let events = backend(AiBackendId::Ollama)
            .parse_chat(r#"{"message":{"role":"assistant","content":"hi"},"done":false}"#);
        assert_eq!(events, vec![NativeChatEvent::Delta("hi".into())]);

        let done =
            backend(AiBackendId::Ollama).parse_chat(r#"{"message":{"content":""},"done":true}"#);
        assert_eq!(done, vec![NativeChatEvent::Finished]);
    }

    #[test]
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
            backend: AiBackendId::OpenAiCompat,
            supports_thinking: false,
            thinking_enabled: false,
            reasoning_effort: String::new(),
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
        let _guard = native_chat_cancel_test_lock();
        request_native_chat_cancel();
        let req = NativeChatRequest {
            base_url: "http://localhost:11434".into(),
            api_key: String::new(),
            model: "qwen2.5-coder".into(),
            messages: vec![],
            backend: AiBackendId::Ollama,
            supports_thinking: false,
            thinking_enabled: false,
            reasoning_effort: String::new(),
        };
        let body = completion_request_body(&req);
        clear_native_chat_cancel();
        assert_eq!(body["options"]["num_predict"], 100);
        assert_eq!(body["options"]["temperature"], 0.2);
        assert_eq!(body["options"]["stop"][0], "\n\n");
    }

    #[test]
    fn completion_token_from_event_ignores_thoughts() {
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Delta("sel".into())),
            Some(CompletionToken::Text("sel".into()))
        );
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Thought("hmm".into())),
            None
        );
        assert_eq!(completion_token_from_event(NativeChatEvent::Finished), None);
        assert_eq!(
            completion_token_from_event(NativeChatEvent::Error("x".into())),
            Some(CompletionToken::Error("x".into()))
        );
    }

    #[tokio::test]
    // Holds the cancel test lock across awaits so parallel tests cannot arm the flag mid-stream.
    #[allow(clippy::await_holding_lock)]
    async fn live_stream_emits_deltas_then_stops_on_cancel() {
        use futures_util::StreamExt as _;

        let _guard = native_chat_cancel_test_lock();
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
}
