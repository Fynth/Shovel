use models::{AiBackendId, AiModelEntry, normalize_native_chat_url};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AiBackend, CompleteRequest, CompletionToken, sql_complete_messages};
use crate::{NativeChatEvent, NativeChatRequest};

pub(super) static INSTANCE: OpenAiCompatBackend = OpenAiCompatBackend;

pub struct OpenAiCompatBackend;

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
struct OpenAiModelList {
    #[serde(default)]
    data: Vec<OpenAiModelListEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelListEntry {
    id: String,
}

enum OpenAiSseEvent {
    Content(String),
    Reasoning(String),
    Done,
    Error(String),
}

fn openai_chat_url(base: &str) -> String {
    let normalized = normalize_native_chat_url(base, base);
    if base.contains("chat/completions") || normalized.contains("chat/completions") {
        normalized
    } else if normalized.ends_with("/v1") {
        format!("{normalized}/chat/completions")
    } else {
        format!("{normalized}/v1/chat/completions")
    }
}

fn normalize_reasoning_effort_value(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

fn parse_openai_sse_events(body: &str) -> Vec<OpenAiSseEvent> {
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
            events.push(OpenAiSseEvent::Done);
            continue;
        }

        let Ok(chunk) = serde_json::from_str::<OpenAiChatChunk>(data) else {
            continue;
        };

        if let Some(error) = chunk.error {
            events.push(OpenAiSseEvent::Error(error.to_string()));
            continue;
        }

        if let Some(choice) = chunk.choices.first() {
            if let Some(reasoning) = choice.delta.reasoning_content.as_ref()
                && !reasoning.is_empty()
            {
                events.push(OpenAiSseEvent::Reasoning(reasoning.clone()));
            }
            if let Some(content) = choice.delta.content.as_ref()
                && !content.is_empty()
            {
                events.push(OpenAiSseEvent::Content(content.clone()));
            }
        }
    }
    events
}

impl AiBackend for OpenAiCompatBackend {
    fn id(&self) -> AiBackendId {
        AiBackendId::OpenAiCompat
    }

    fn chat_url(&self, base: &str) -> Result<String, String> {
        Ok(openai_chat_url(base))
    }

    fn complete_url(&self, base: &str) -> Result<String, String> {
        self.chat_url(base)
    }

    fn models_url(&self, base: &str) -> Result<String, String> {
        let normalized = normalize_native_chat_url(base, base);
        if normalized.ends_with("/v1") {
            Ok(format!("{normalized}/models"))
        } else {
            Ok(format!("{normalized}/v1/models"))
        }
    }

    fn chat_body(&self, req: &NativeChatRequest) -> Result<Value, String> {
        let mut body = json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
        });

        if req.supports_thinking {
            let obj = body.as_object_mut().expect("request body is object");
            if req.thinking_enabled {
                obj.insert("thinking".into(), json!({ "type": "enabled" }));
            }
            obj.insert(
                "reasoning_effort".into(),
                Value::String(normalize_reasoning_effort_value(&req.reasoning_effort).to_string()),
            );
        }

        Ok(body)
    }

    fn complete_body(&self, req: &CompleteRequest) -> Result<Value, String> {
        Ok(json!({
            "model": req.model,
            "messages": sql_complete_messages(req),
            "max_tokens": 100,
            "temperature": 0.1,
            "stop": ["\n\n", ";", "```"],
            "stream": true,
        }))
    }

    fn parse_chat(&self, payload: &str) -> Vec<NativeChatEvent> {
        parse_openai_sse_events(payload)
            .into_iter()
            .map(|event| match event {
                OpenAiSseEvent::Content(text) => NativeChatEvent::Delta(text),
                OpenAiSseEvent::Reasoning(text) => NativeChatEvent::Thought(text),
                OpenAiSseEvent::Done => NativeChatEvent::Finished,
                OpenAiSseEvent::Error(text) => NativeChatEvent::Error(text),
            })
            .collect()
    }

    fn parse_complete(&self, payload: &str) -> Vec<CompletionToken> {
        parse_openai_sse_events(payload)
            .into_iter()
            .filter_map(|event| match event {
                OpenAiSseEvent::Content(text) => Some(CompletionToken::Text(text)),
                OpenAiSseEvent::Reasoning(_) => None,
                OpenAiSseEvent::Done => Some(CompletionToken::Done),
                OpenAiSseEvent::Error(text) => Some(CompletionToken::Error(text)),
            })
            .collect()
    }

    fn parse_models(&self, json: &str) -> Result<Vec<AiModelEntry>, String> {
        let list: OpenAiModelList = serde_json::from_str(json)
            .map_err(|err| format!("Failed to parse OpenAI model list: {err}"))?;
        Ok(list
            .data
            .into_iter()
            .map(|entry| AiModelEntry {
                id: entry.id,
                label: String::new(),
            })
            .collect())
    }
}
