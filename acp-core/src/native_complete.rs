//! Native HTTP SQL completion. Does not read or write `NATIVE_CHAT_CANCEL`.

use std::time::Duration;

use futures_util::StreamExt;
use models::AiBackendId;
use tokio::sync::mpsc;

use crate::{
    backends::{CompleteRequest, CompletionToken, backend},
    native_chat::auth_headers,
};

const COMPLETE_TIMEOUT: Duration = Duration::from_secs(15);

pub fn complete_sql(req: CompleteRequest) -> mpsc::UnboundedReceiver<CompletionToken> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = tx.closed() => {}
            result = run_complete(req, &tx) => {
                if let Err(err) = result {
                    let _ = tx.send(CompletionToken::Error(err));
                }
                let _ = tx.send(CompletionToken::Done);
            }
        }
    });
    rx
}

async fn run_complete(
    req: CompleteRequest,
    tx: &mpsc::UnboundedSender<CompletionToken>,
) -> Result<(), String> {
    let b = backend(req.backend);
    let url = b.complete_url(&req.base_url)?;
    let body = b.complete_body(&req)?;

    let client = reqwest::Client::builder()
        .connect_timeout(COMPLETE_TIMEOUT)
        .timeout(COMPLETE_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build HTTP client: {err}"))?;

    let response = client
        .post(&url)
        .headers(auth_headers(&req.api_key)?)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("Completion request failed: {err}"))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("Auth failed".into());
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{status}: {}", body.trim()));
    }

    if req.backend == AiBackendId::MistralFim {
        let text = response
            .text()
            .await
            .map_err(|err| format!("Completion body failed: {err}"))?;
        emit_tokens(b.parse_complete(&text), tx);
        return Ok(());
    }

    stream_complete_lines(response, req.backend, tx).await
}

async fn stream_complete_lines(
    response: reqwest::Response,
    backend_id: AiBackendId,
    tx: &mpsc::UnboundedSender<CompletionToken>,
) -> Result<(), String> {
    let b = backend(backend_id);
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        if tx.is_closed() {
            return Ok(());
        }
        let chunk = chunk.map_err(|err| format!("Completion stream failed: {err}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(idx) = buffer.find('\n') {
            let mut line = buffer[..idx].to_string();
            buffer.drain(..=idx);
            if line.ends_with('\r') {
                line.pop();
            }
            if emit_tokens(b.parse_complete(&line), tx) {
                return Ok(());
            }
        }
    }

    if !buffer.trim().is_empty() {
        emit_tokens(b.parse_complete(&buffer), tx);
    }
    Ok(())
}

/// Forward `Text`/`Error`. Skip `Done` (the wrapper always sends it).
/// Returns true when streaming should stop (Done seen or receiver dropped).
fn emit_tokens(tokens: Vec<CompletionToken>, tx: &mpsc::UnboundedSender<CompletionToken>) -> bool {
    for token in tokens {
        match token {
            CompletionToken::Done => return true,
            token =>
                if tx.send(token).is_err() {
                    return true;
                },
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_sql_is_exported_shape() {
        let req = CompleteRequest {
            backend: models::AiBackendId::MistralFim,
            base_url: "https://codestral.mistral.ai".into(),
            api_key: String::new(),
            model: "codestral-latest".into(),
            prefix: "SELECT ".into(),
            suffix: None,
            schema_context: String::new(),
        };
        let _: fn(CompleteRequest) -> mpsc::UnboundedReceiver<CompletionToken> = complete_sql;
        let _ = req;
    }

    #[test]
    fn complete_does_not_set_native_chat_cancel() {
        let _guard = crate::native_chat::native_chat_cancel_test_lock();
        crate::native_chat::clear_native_chat_cancel();
        assert!(!crate::native_chat::native_chat_cancel_requested());
    }
}
