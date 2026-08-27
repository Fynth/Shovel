use models::{AiBackendId, AiModelEntry, normalize_native_chat_url};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AiBackend, CompleteRequest, CompletionToken, unsupported};
use crate::{NativeChatEvent, NativeChatRequest};

pub(super) static INSTANCE: MistralFimBackend = MistralFimBackend;

pub struct MistralFimBackend;

#[derive(Debug, Deserialize)]
struct FimResponse {
    #[serde(default)]
    choices: Vec<FimChoice>,
}

#[derive(Debug, Deserialize)]
struct FimChoice {
    text: Option<String>,
    message: Option<FimMessage>,
}

#[derive(Debug, Deserialize)]
struct FimMessage {
    content: Option<String>,
}

impl AiBackend for MistralFimBackend {
    fn id(&self) -> AiBackendId {
        AiBackendId::MistralFim
    }

    fn chat_url(&self, _base: &str) -> Result<String, String> {
        Err(unsupported("chat"))
    }

    fn complete_url(&self, base: &str) -> Result<String, String> {
        let normalized = normalize_native_chat_url(base, base);
        if base.contains("fim/completions") || normalized.contains("fim/completions") {
            Ok(normalized)
        } else {
            Ok(format!("{normalized}/v1/fim/completions"))
        }
    }

    fn models_url(&self, _base: &str) -> Result<String, String> {
        Err(unsupported("list_models"))
    }

    fn chat_body(&self, _req: &NativeChatRequest) -> Result<Value, String> {
        Err(unsupported("chat"))
    }

    fn complete_body(&self, req: &CompleteRequest) -> Result<Value, String> {
        let prompt = if req.schema_context.is_empty() {
            req.prefix.clone()
        } else {
            format!(
                "-- Database schema:\n{}\n\n{}",
                req.schema_context, req.prefix
            )
        };
        Ok(json!({
            "model": req.model,
            "prompt": prompt,
            "suffix": req.suffix.clone().unwrap_or_default(),
            "max_tokens": 80,
            "temperature": 0.2,
            "top_p": 0.95,
            "stop": ["\n\n", ";"],
        }))
    }

    fn parse_chat(&self, _payload: &str) -> Vec<NativeChatEvent> {
        vec![NativeChatEvent::Error(unsupported("chat"))]
    }

    fn parse_complete(&self, payload: &str) -> Vec<CompletionToken> {
        let Ok(resp) = serde_json::from_str::<FimResponse>(payload) else {
            return vec![CompletionToken::Error(
                "Failed to parse FIM completion response".into(),
            )];
        };
        let text = resp
            .choices
            .first()
            .and_then(|choice| {
                choice
                    .text
                    .clone()
                    .or_else(|| choice.message.as_ref().and_then(|msg| msg.content.clone()))
            })
            .unwrap_or_default();
        let text = text.trim_matches(['\r', '\n']).to_string();
        if text.is_empty() {
            vec![CompletionToken::Done]
        } else {
            vec![CompletionToken::Text(text), CompletionToken::Done]
        }
    }

    fn parse_models(&self, _json: &str) -> Result<Vec<AiModelEntry>, String> {
        Err(unsupported("list_models"))
    }
}
