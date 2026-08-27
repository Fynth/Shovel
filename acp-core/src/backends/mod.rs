mod mistral_fim;
mod ollama;
mod openai;

use models::{AiBackendId, AiCapabilities, backend_capabilities};
use serde_json::Value;

use crate::{NativeChatEvent, NativeChatRequest};

pub use mistral_fim::MistralFimBackend;
pub use ollama::OllamaBackend;
pub use openai::OpenAiCompatBackend;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionToken {
    Text(String),
    Done,
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompleteRequest {
    pub backend: AiBackendId,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub prefix: String,
    pub suffix: Option<String>,
    pub schema_context: String,
}

pub trait AiBackend: Send + Sync {
    fn id(&self) -> AiBackendId;
    fn capabilities(&self) -> AiCapabilities {
        backend_capabilities(self.id())
    }
    fn chat_url(&self, base: &str) -> Result<String, String>;
    fn complete_url(&self, base: &str) -> Result<String, String>;
    fn models_url(&self, base: &str) -> Result<String, String>;
    fn chat_body(&self, req: &NativeChatRequest) -> Result<Value, String>;
    fn complete_body(&self, req: &CompleteRequest) -> Result<Value, String>;
    fn parse_chat(&self, payload: &str) -> Vec<NativeChatEvent>;
    fn parse_complete(&self, payload: &str) -> Vec<CompletionToken>;
    fn parse_models(&self, json: &str) -> Result<Vec<models::AiModelEntry>, String>;
}

pub fn backend(id: AiBackendId) -> &'static dyn AiBackend {
    match id {
        AiBackendId::OpenAiCompat => &openai::INSTANCE,
        AiBackendId::Ollama => &ollama::INSTANCE,
        AiBackendId::MistralFim => &mistral_fim::INSTANCE,
    }
}

fn unsupported(op: &str) -> String {
    format!("{op} is not supported by this backend")
}

pub(super) fn sql_complete_messages(req: &CompleteRequest) -> Vec<crate::NativeChatMessage> {
    let schema_part = if req.schema_context.is_empty() {
        String::new()
    } else {
        format!("Database schema:\n{}\n\n", req.schema_context)
    };
    let prefix = req.prefix.as_str();
    let system = format!(
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
    let user = match req.suffix.as_deref() {
        Some(suffix) => {
            format!("Complete between [CURSOR]:\n```sql\n{prefix}[CURSOR]{suffix}\n```")
        }
        None => format!("Complete after [CURSOR]:\n```sql\n{prefix}[CURSOR]\n```"),
    };
    vec![
        crate::NativeChatMessage {
            role: "system".into(),
            content: system,
        },
        crate::NativeChatMessage {
            role: "user".into(),
            content: user,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::AiBackendId;

    fn openai_chat_req() -> crate::NativeChatRequest {
        crate::NativeChatRequest {
            base_url: "https://api.openai.com/".into(),
            api_key: "sk".into(),
            model: "gpt-4o-mini".into(),
            messages: vec![crate::NativeChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            backend: AiBackendId::OpenAiCompat,
            supports_thinking: false,
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        }
    }

    #[test]
    fn openai_chat_url_does_not_double_v1() {
        let b = backend(AiBackendId::OpenAiCompat);
        assert_eq!(
            b.chat_url("https://api.openai.com/").unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            b.chat_url("https://example.com/v1/chat/completions")
                .unwrap(),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(
            b.chat_url("https://api.minimax.chat/v1").unwrap(),
            "https://api.minimax.chat/v1/chat/completions"
        );
    }

    #[test]
    fn thinking_fields_follow_flag_not_slug() {
        let b = backend(AiBackendId::OpenAiCompat);
        let mut req = openai_chat_req();
        req.supports_thinking = true;
        req.thinking_enabled = true;
        req.reasoning_effort = "high".into();
        let v = b.chat_body(&req).unwrap();
        assert_eq!(v["thinking"]["type"], "enabled");
        assert_eq!(v["reasoning_effort"], "high");

        req.supports_thinking = false;
        let v = b.chat_body(&req).unwrap();
        assert!(v.get("thinking").is_none());
        assert!(v.get("reasoning_effort").is_none());
    }

    #[test]
    fn ollama_urls() {
        let b = backend(AiBackendId::Ollama);
        assert_eq!(
            b.chat_url("http://localhost:11434").unwrap(),
            "http://localhost:11434/api/chat"
        );
        assert_eq!(
            b.chat_url("http://localhost:11434/api").unwrap(),
            "http://localhost:11434/api/chat"
        );
        assert_eq!(
            b.models_url("http://localhost:11434").unwrap(),
            "http://localhost:11434/api/tags"
        );
    }

    #[test]
    fn mistral_fim_complete_url_and_no_chat() {
        let b = backend(AiBackendId::MistralFim);
        assert!(b.chat_url("https://codestral.mistral.ai").is_err());
        assert!(b.models_url("https://codestral.mistral.ai").is_err());
        assert_eq!(
            b.complete_url("https://codestral.mistral.ai").unwrap(),
            "https://codestral.mistral.ai/v1/fim/completions"
        );
        assert_eq!(
            b.complete_url("https://codestral.mistral.ai/v1/fim/completions")
                .unwrap(),
            "https://codestral.mistral.ai/v1/fim/completions"
        );
    }

    #[test]
    fn openai_complete_body_is_chat_sql_prompt() {
        let b = backend(AiBackendId::OpenAiCompat);
        let req = CompleteRequest {
            backend: AiBackendId::OpenAiCompat,
            base_url: "https://api.deepseek.com".into(),
            api_key: "sk".into(),
            model: "deepseek-chat".into(),
            prefix: "SELECT ".into(),
            suffix: None,
            schema_context: String::new(),
        };
        let v = b.complete_body(&req).unwrap();
        assert_eq!(v["model"], "deepseek-chat");
        assert_eq!(v["stream"], true);
        assert_eq!(v["max_tokens"], 100);
        let msgs = v["messages"].as_array().unwrap();
        assert!(
            msgs[0]["content"]
                .as_str()
                .unwrap()
                .contains("SQL autocomplete")
        );
        assert!(msgs[1]["content"].as_str().unwrap().contains("[CURSOR]"));
    }

    #[test]
    fn openai_complete_parse_skips_reasoning() {
        let b = backend(AiBackendId::OpenAiCompat);
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\",\"content\":\"SEL\"}}]}\n",
            "data: [DONE]\n",
        );
        let events = b.parse_complete(body);
        assert_eq!(
            events,
            vec![CompletionToken::Text("SEL".into()), CompletionToken::Done,]
        );
    }

    #[test]
    fn mistral_fim_parse_one_shot() {
        let b = backend(AiBackendId::MistralFim);
        let json = r#"{"choices":[{"text":"id FROM t"}]}"#;
        assert_eq!(
            b.parse_complete(json),
            vec![
                CompletionToken::Text("id FROM t".into()),
                CompletionToken::Done
            ]
        );
    }
}
