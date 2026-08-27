//! Native HTTP chat prompt, model refresh, and shared cancel wiring.

use futures_util::StreamExt;
use models::{AcpEvent, AcpMessageKind, AiModelEntry, normalize_native_chat_url};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::{
    native_chat::{
        NativeChatEvent,
        NativeChatRequest,
        clear_native_chat_cancel,
        native_chat_cancel_requested,
        stream_native_chat,
    },
    runtime::push_acp_event,
};

#[derive(Debug, Deserialize)]
struct OpenAiModelList {
    #[serde(default)]
    data: Vec<OpenAiModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
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

/// Parse OpenAI-compatible `GET /v1/models` JSON into model ids.
pub fn parse_openai_model_list(json: &str) -> Result<Vec<String>, String> {
    let list: OpenAiModelList = serde_json::from_str(json)
        .map_err(|err| format!("Failed to parse OpenAI model list: {err}"))?;
    Ok(list.data.into_iter().map(|entry| entry.id).collect())
}

/// Parse Ollama `GET /api/tags` JSON into model names.
pub fn parse_ollama_tag_list(json: &str) -> Result<Vec<String>, String> {
    let list: OllamaTagList = serde_json::from_str(json)
        .map_err(|err| format!("Failed to parse Ollama tag list: {err}"))?;
    Ok(list.models.into_iter().map(|entry| entry.name).collect())
}

fn models_url(slug: &str, base_url: &str) -> String {
    let normalized = normalize_native_chat_url(base_url, base_url);
    if slug == "ollama" {
        if normalized.ends_with("/api") {
            format!("{normalized}/tags")
        } else {
            format!("{normalized}/api/tags")
        }
    } else if normalized.ends_with("/v1") {
        format!("{normalized}/models")
    } else {
        format!("{normalized}/v1/models")
    }
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

/// Refresh the model list for a native provider (OpenAI-compat or Ollama).
pub async fn refresh_provider_models(
    slug: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<AiModelEntry>, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let url = models_url(slug, base_url);
    let response = client
        .get(&url)
        .headers(auth_headers(api_key)?)
        .send()
        .await
        .map_err(|err| format!("Model refresh request failed: {err}"))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("Auth failed".into());
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{status}: {}", body.trim()));
    }

    let body = response
        .text()
        .await
        .map_err(|err| format!("Model refresh body failed: {err}"))?;

    let ids = if slug == "ollama" {
        parse_ollama_tag_list(&body)?
    } else {
        parse_openai_model_list(&body)?
    };

    Ok(ids
        .into_iter()
        .map(|id| AiModelEntry {
            id,
            label: String::new(),
        })
        .collect())
}

fn map_native_event(event: NativeChatEvent) -> AcpEvent {
    match event {
        NativeChatEvent::Delta(text) => AcpEvent::Message {
            kind: AcpMessageKind::Agent,
            text,
        },
        NativeChatEvent::Thought(text) => AcpEvent::Message {
            kind: AcpMessageKind::Thought,
            text,
        },
        NativeChatEvent::Finished => AcpEvent::PromptFinished {
            stop_reason: "EndTurn".into(),
        },
        NativeChatEvent::Error(text) => AcpEvent::Error(text),
    }
}

/// Run a native HTTP chat completion and push results onto the shared ACP event queue.
pub async fn native_chat_prompt(req: NativeChatRequest) -> Result<(), String> {
    clear_native_chat_cancel();
    push_acp_event(AcpEvent::PromptStarted);

    let stream = match stream_native_chat(req).await {
        Ok(stream) => stream,
        Err(err) => {
            push_acp_event(AcpEvent::Error(err.clone()));
            return Err(err);
        }
    };

    futures_util::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        if native_chat_cancel_requested() {
            push_acp_event(AcpEvent::PromptFinished {
                stop_reason: "Cancelled".into(),
            });
            return Ok(());
        }

        let is_terminal = matches!(event, NativeChatEvent::Finished | NativeChatEvent::Error(_));
        push_acp_event(map_native_event(event));
        if is_terminal {
            return Ok(());
        }
    }

    push_acp_event(AcpEvent::PromptFinished {
        stop_reason: "EndTurn".into(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_openai_model_list() {
        let json = r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
        let ids = super::parse_openai_model_list(json).unwrap();
        assert_eq!(ids, ["gpt-4o", "gpt-4o-mini"]);
    }

    #[test]
    fn parse_ollama_tag_list() {
        let json = r#"{"models":[{"name":"qwen3:latest"}]}"#;
        let ids = super::parse_ollama_tag_list(json).unwrap();
        assert_eq!(ids, ["qwen3:latest"]);
    }
}
