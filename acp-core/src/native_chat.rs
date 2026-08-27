//! In-process OpenAI-compatible and Ollama native chat streaming.

use futures_util::stream::{self, Stream};
use models::normalize_native_chat_url;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

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

/// Parse an Ollama NDJSON stream body into chat events.
fn parse_ollama_ndjson(body: &str) -> Vec<NativeChatEvent> {
    let mut events = Vec::new();
    for line in body.lines() {
        events.extend(parse_ollama_ndjson_line(line));
    }
    if !events
        .iter()
        .any(|e| matches!(e, NativeChatEvent::Finished | NativeChatEvent::Error(_)))
    {
        events.push(NativeChatEvent::Finished);
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

/// POST to the provider chat endpoint and yield parsed events.
pub async fn stream_native_chat(
    req: NativeChatRequest,
) -> Result<impl Stream<Item = NativeChatEvent>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let url = chat_url(&req);
    let is_ollama = req.provider_slug == "ollama";
    let body = if is_ollama {
        ollama_request_body(&req)
    } else {
        openai_request_body(&req)
    };

    let response = client
        .post(&url)
        .headers(auth_headers(&req.api_key)?)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("Native chat request failed: {err}"))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Ok(stream::iter(vec![NativeChatEvent::Error(
            "Auth failed".into(),
        )]));
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Ok(stream::iter(vec![NativeChatEvent::Error(format!(
            "{status}: {}",
            body.trim()
        ))]));
    }

    let text = response
        .text()
        .await
        .map_err(|err| format!("Native chat stream failed: {err}"))?;

    let events = if is_ollama {
        parse_ollama_ndjson(&text)
    } else {
        let mut events = parse_openai_sse(&text);
        if !events
            .iter()
            .any(|e| matches!(e, NativeChatEvent::Finished | NativeChatEvent::Error(_)))
        {
            events.push(NativeChatEvent::Finished);
        }
        events
    };

    Ok(stream::iter(events))
}

#[cfg(test)]
mod tests {
    use super::{
        NativeChatEvent,
        NativeChatMessage,
        NativeChatRequest,
        chat_url,
        openai_request_body,
        parse_ollama_ndjson_line,
        parse_openai_sse,
    };

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
}
