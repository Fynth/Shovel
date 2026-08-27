//! Native HTTP chat prompt, model refresh, and shared cancel wiring.

use futures_util::StreamExt;
use models::{AcpEvent, AcpMessageKind, AiModelEntry};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

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
    backend_id: models::AiBackendId,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<AiModelEntry>, String> {
    let backend = crate::backends::backend(backend_id);
    let url = backend.models_url(base_url)?;
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

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

    backend.parse_models(&body)
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
    fn models_url_comes_from_backend() {
        use crate::backends::backend;
        use models::AiBackendId;
        assert_eq!(
            backend(AiBackendId::OpenAiCompat)
                .models_url("https://api.openai.com")
                .unwrap(),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            backend(AiBackendId::Ollama)
                .models_url("http://localhost:11434")
                .unwrap(),
            "http://localhost:11434/api/tags"
        );
        assert!(
            backend(AiBackendId::MistralFim)
                .models_url("https://codestral.mistral.ai")
                .is_err()
        );
    }

    #[test]
    fn parse_models_openai_and_ollama() {
        use crate::backends::backend;
        use models::AiBackendId;
        let openai = backend(AiBackendId::OpenAiCompat)
            .parse_models(r#"{"data":[{"id":"gpt-4o"}]}"#)
            .unwrap();
        assert_eq!(openai[0].id, "gpt-4o");
        let ollama = backend(AiBackendId::Ollama)
            .parse_models(r#"{"models":[{"name":"llama3"}]}"#)
            .unwrap();
        assert_eq!(ollama[0].id, "llama3");
    }
}
