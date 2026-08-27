use models::{AiBackendId, AiModelEntry, normalize_native_chat_url};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AiBackend, CompleteRequest, CompletionToken, sql_complete_messages};
use crate::{NativeChatEvent, NativeChatRequest};

pub(super) static INSTANCE: OllamaBackend = OllamaBackend;

pub struct OllamaBackend;

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

#[derive(Debug, Deserialize)]
struct OllamaTagList {
    #[serde(default)]
    models: Vec<OllamaTagEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagEntry {
    name: String,
}

fn ollama_api_url(base: &str, root_suffix: &str, api_suffix: &str) -> String {
    let normalized = normalize_native_chat_url(base, base);
    if normalized.ends_with("/api") {
        format!("{normalized}/{root_suffix}")
    } else {
        format!("{normalized}/api/{api_suffix}")
    }
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

impl AiBackend for OllamaBackend {
    fn id(&self) -> AiBackendId {
        AiBackendId::Ollama
    }

    fn chat_url(&self, base: &str) -> Result<String, String> {
        Ok(ollama_api_url(base, "chat", "chat"))
    }

    fn complete_url(&self, base: &str) -> Result<String, String> {
        self.chat_url(base)
    }

    fn models_url(&self, base: &str) -> Result<String, String> {
        Ok(ollama_api_url(base, "tags", "tags"))
    }

    fn chat_body(&self, req: &NativeChatRequest) -> Result<Value, String> {
        Ok(json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
        }))
    }

    fn complete_body(&self, req: &CompleteRequest) -> Result<Value, String> {
        Ok(json!({
            "model": req.model,
            "messages": sql_complete_messages(req),
            "stream": true,
        }))
    }

    fn parse_chat(&self, payload: &str) -> Vec<NativeChatEvent> {
        payload.lines().flat_map(parse_ollama_ndjson_line).collect()
    }

    fn parse_complete(&self, payload: &str) -> Vec<CompletionToken> {
        let mut events = Vec::new();
        for line in payload.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<OllamaChatChunk>(line) else {
                continue;
            };
            if let Some(error) = chunk.error {
                events.push(CompletionToken::Error(error));
                continue;
            }
            if let Some(message) = chunk.message
                && !message.content.is_empty()
            {
                events.push(CompletionToken::Text(message.content));
            }
            if chunk.done {
                events.push(CompletionToken::Done);
            }
        }
        events
    }

    fn parse_models(&self, json: &str) -> Result<Vec<AiModelEntry>, String> {
        let list: OllamaTagList = serde_json::from_str(json)
            .map_err(|err| format!("Failed to parse Ollama tag list: {err}"))?;
        Ok(list
            .models
            .into_iter()
            .map(|entry| AiModelEntry {
                id: entry.name,
                label: String::new(),
            })
            .collect())
    }
}
