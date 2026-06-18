//! AI-powered SQL completion with multi-provider fallback and streaming.
//!
//! Providers are tried in order:
//! 1. DeepSeek (chat API with streaming) — real-time token output
//! 2. CodeStral (Mistral FIM API) — fast, purpose-built for code completion
//!
//! Streaming completions appear incrementally as the model generates tokens,
//! giving instant feedback like Zed's inline assistant.

use futures_util::StreamExt;
use models::{AppUiSettings, CodeStralSettings, DeepSeekSettings};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::mpsc;

// ── Shared HTTP client ──────────────────────────────────────────────

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn http_client() -> Client {
    HTTP_CLIENT.get_or_init(Client::new).clone()
}

// ── Streaming completion token ──────────────────────────────────────

/// A token emitted during streaming completion.
#[derive(Clone, Debug)]
pub enum CompletionToken {
    /// A piece of completion text (may be partial).
    Text(String),
    /// Streaming finished successfully.
    Done,
    /// Streaming failed with an error.
    Error(String),
}

// ── Completion service ──────────────────────────────────────────────

enum CompletionProvider {
    CodeStral(CodeStralProvider),
    DeepSeek(DeepSeekProvider),
}

pub struct CompletionService {
    providers: Vec<CompletionProvider>,
}

impl CompletionService {
    pub fn new(settings: &AppUiSettings) -> Self {
        let mut providers: Vec<CompletionProvider> = Vec::new();

        // DeepSeek first — supports streaming, better for real-time UX.
        if settings.deepseek.enabled && !settings.deepseek.api_key.is_empty() {
            providers.push(CompletionProvider::DeepSeek(DeepSeekProvider::new(
                settings.deepseek.clone(),
            )));
        }

        if settings.codestral.enabled && !settings.codestral.api_key.is_empty() {
            providers.push(CompletionProvider::CodeStral(CodeStralProvider::new(
                settings.codestral.clone(),
            )));
        }

        Self { providers }
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Stream a completion from the first available provider.
    /// Returns a receiver that yields [`CompletionToken`] values.
    pub fn stream_completion(
        &self,
        prefix: String,
        suffix: Option<String>,
        schema_context: String,
    ) -> mpsc::UnboundedReceiver<CompletionToken> {
        let (tx, rx) = mpsc::unbounded_channel();

        if self.providers.is_empty() {
            let _ = tx.send(CompletionToken::Done);
            return rx;
        }

        // Extract provider configs for the async task.
        let codestral = self.providers.iter().find_map(|p| match p {
            CompletionProvider::CodeStral(c) => Some((c.client.clone(), c.settings.clone())),
            _ => None,
        });
        let deepseek = self.providers.iter().find_map(|p| match p {
            CompletionProvider::DeepSeek(d) => Some((d.client.clone(), d.settings.clone())),
            _ => None,
        });

        tokio::task::spawn(async move {
            let mut attempts = 0u32;
            let mut successes = 0u32;
            let mut errors: Vec<String> = Vec::new();

            // Try DeepSeek first (streaming, better UX).
            if let Some((client, settings)) = deepseek {
                attempts += 1;
                match stream_deepseek(
                    &client,
                    &settings,
                    &prefix,
                    suffix.as_deref(),
                    &schema_context,
                    &tx,
                )
                .await
                {
                    Ok(()) => {
                        let _ = tx.send(CompletionToken::Done);
                        return;
                    }
                    Err(e) => {
                        eprintln!("[completion] DeepSeek error: {e}");
                        errors.push(format!("DeepSeek: {e}"));
                    }
                }
            }

            // Fall back to CodeStral (single-shot).
            if let Some((client, settings)) = codestral {
                attempts += 1;
                match codestral_complete(
                    &client,
                    &settings,
                    &prefix,
                    suffix.as_deref(),
                    &schema_context,
                )
                .await
                {
                    Ok(Some(text)) => {
                        successes += 1;
                        let _ = tx.send(CompletionToken::Text(text));
                    }
                    Ok(None) => {
                        successes += 1;
                    }
                    Err(e) => {
                        eprintln!("[completion] CodeStral error: {e}");
                        errors.push(format!("CodeStral: {e}"));
                    }
                }
            }

            // If every attempted provider failed, surface the error.
            if attempts > 0 && successes == 0 {
                let msg = if errors.len() == 1 {
                    format!("AI completion failed: {}", errors[0])
                } else {
                    format!(
                        "AI completion failed ({} providers): {}",
                        errors.len(),
                        errors.join("; ")
                    )
                };
                let _ = tx.send(CompletionToken::Error(msg));
            }

            let _ = tx.send(CompletionToken::Done);
        });

        rx
    }
}

// ── DeepSeek streaming ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct DeepSeekStreamRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    max_tokens: usize,
    temperature: f32,
    stop: Vec<String>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct DeepSeekMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekStreamChunk {
    choices: Vec<DeepSeekStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekStreamChoice {
    delta: Option<DeepSeekStreamDelta>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekStreamDelta {
    content: Option<String>,
    // `reasoning_content` is part of the DeepSeek wire format but
    // is currently consumed only as part of `content` for the
    // completion feature. The struct omits the field so the
    // deserializer silently drops it; if we ever want to surface
    // chain-of-thought we can re-add it here.
}

async fn stream_deepseek(
    client: &Client,
    settings: &DeepSeekSettings,
    prefix: &str,
    suffix: Option<&str>,
    schema_context: &str,
    tx: &mpsc::UnboundedSender<CompletionToken>,
) -> Result<(), String> {
    let schema_part = if schema_context.is_empty() {
        String::new()
    } else {
        format!("Database schema:\n{schema_context}\n\n")
    };

    let system_prompt = format!(
        "You are a SQL autocomplete engine inside a database client.\n\
         Your task: given the SQL before the cursor and the database schema,\n\
         output ONLY the SQL text that should come next.\n\n\
         RULES:\n\
         1. Output ONLY raw SQL — no markdown, no backticks, no explanations.\n\
         2. Match the existing SQL style (keywords case, indentation).\n\
         3. Use the schema to suggest correct table/column names.\n\
         4. If the statement is already complete, output nothing.\n\
         5. Do NOT repeat what's already typed before or after the cursor.\n\n\
         {schema_part}\
         Surrounding SQL context (before cursor):\n\
         ```sql\n{prefix}\n```",
    );

    let user_prompt = if let Some(suffix) = suffix {
        format!("Complete between [CURSOR]:\n```sql\n{prefix}[CURSOR]{suffix}\n```")
    } else {
        format!("Complete after [CURSOR]:\n```sql\n{prefix}[CURSOR]\n```")
    };

    let request = DeepSeekStreamRequest {
        model: settings.model.clone(),
        messages: vec![
            DeepSeekMessage {
                role: "system".to_string(),
                content: system_prompt,
            },
            DeepSeekMessage {
                role: "user".to_string(),
                content: user_prompt,
            },
        ],
        max_tokens: 100,
        temperature: 0.1,
        stop: vec!["\n\n".to_string(), ";".to_string(), "```".to_string()],
        stream: true,
    };

    let api_url = format!("{}/v1/chat/completions", settings.base_url);

    let response = client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("DeepSeek network error: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("DeepSeek API {}: {}", status.as_u16(), body));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE lines.
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let data = line.strip_prefix("data: ").unwrap_or(&line);
            if data == "[DONE]" {
                return Ok(());
            }

            if let Ok(chunk) = serde_json::from_str::<DeepSeekStreamChunk>(data)
                && let Some(choice) = chunk.choices.first()
                && let Some(delta) = &choice.delta
            {
                // Only collect actual `content`, not the model's reasoning/thinking.
                // Reasoning models (v4-flash) spend tokens on chain-of-thought first,
                // then produce the real answer in `content`.
                if let Some(content) = &delta.content
                    && !content.is_empty()
                {
                    let _ = tx.send(CompletionToken::Text(content.clone()));
                }
            }
        }
    }

    Ok(())
}

// ── CodeStral single-shot (no streaming API) ────────────────────────

const CODESTRAL_API_URL: &str = "https://codestral.mistral.ai/v1/fim/completions";

#[derive(Debug, Serialize)]
struct CodeStralRequest {
    model: String,
    prompt: String,
    suffix: Option<String>,
    max_tokens: usize,
    temperature: f32,
    top_p: f32,
    stop: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CodeStralResponse {
    choices: Vec<CodeStralChoice>,
}

#[derive(Debug, Deserialize)]
struct CodeStralChoice {
    text: Option<String>,
    message: Option<CodeStralMessage>,
}

#[derive(Debug, Deserialize)]
struct CodeStralMessage {
    content: Option<String>,
}

struct CodeStralProvider {
    client: Client,
    settings: CodeStralSettings,
}

impl CodeStralProvider {
    fn new(settings: CodeStralSettings) -> Self {
        Self {
            client: http_client(),
            settings,
        }
    }
}

struct DeepSeekProvider {
    client: Client,
    settings: DeepSeekSettings,
}

impl DeepSeekProvider {
    fn new(settings: DeepSeekSettings) -> Self {
        Self {
            client: http_client(),
            settings,
        }
    }
}

async fn codestral_complete(
    client: &Client,
    settings: &CodeStralSettings,
    prefix: &str,
    suffix: Option<&str>,
    schema_context: &str,
) -> Result<Option<String>, String> {
    let prompt = if schema_context.is_empty() {
        prefix.to_string()
    } else {
        format!("-- Database schema:\n{schema_context}\n\n{prefix}")
    };

    let request = CodeStralRequest {
        model: settings.model.clone(),
        prompt,
        suffix: suffix.map(String::from),
        max_tokens: 80,
        temperature: 0.2,
        top_p: 0.95,
        stop: vec!["\n\n".to_string(), ";".to_string()],
    };

    let response = client
        .post(CODESTRAL_API_URL)
        .header("Authorization", format!("Bearer {}", settings.api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("CodeStral network error: {e}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("CodeStral API {}: {}", status.as_u16(), body));
    }

    let completion: CodeStralResponse =
        serde_json::from_str(&body).map_err(|e| format!("CodeStral parse error: {e}"))?;

    Ok(completion.choices.first().and_then(|c| {
        c.text.as_deref().map(normalize_text).or_else(|| {
            c.message
                .as_ref()?
                .content
                .as_ref()
                .map(|c| normalize_text(c))
        })
    }))
}

// ── Helpers ─────────────────────────────────────────────────────────

fn normalize_text(text: &str) -> String {
    text.trim_matches(|ch| matches!(ch, '\r' | '\n'))
        .trim()
        .to_string()
}
