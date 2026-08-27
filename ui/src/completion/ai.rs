use models::{AppUiSettings, builtin_providers, normalize_native_chat_url, provider_backend};
use services::{CompletionToken, NativeChatMessage, NativeChatRequest};

pub fn stream_sql_ghost(
    settings: &AppUiSettings,
    prefix: String,
    suffix: Option<String>,
    schema_context: String,
    avoid: &[String],
) -> tokio::sync::mpsc::UnboundedReceiver<CompletionToken> {
    let provider_slug = settings.sql_completion.provider.clone();
    let Some(backend) = provider_backend(&provider_slug, &settings.ai_catalog)
        .filter(|_| settings.sql_ghost_ready())
    else {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(CompletionToken::Done);
        return rx;
    };

    let req = NativeChatRequest {
        base_url: ghost_base_url(settings, &provider_slug),
        api_key: settings.lm_api_key(&provider_slug),
        model: settings.sql_completion.model.clone(),
        messages: ghost_messages(&prefix, suffix.as_deref(), &schema_context, avoid),
        backend,
        supports_thinking: builtin_providers()
            .iter()
            .find(|spec| spec.slug == provider_slug)
            .is_some_and(|spec| spec.supports_thinking),
        thinking_enabled: false,
        reasoning_effort: String::new(),
    };
    services::stream_native_completion(req)
}

fn ghost_base_url(settings: &AppUiSettings, slug: &str) -> String {
    let default = settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == slug)
        .map(|custom| custom.base_url.as_str())
        .or_else(|| {
            builtin_providers()
                .iter()
                .find(|spec| spec.slug == slug)
                .map(|spec| spec.default_base_url)
        })
        .unwrap_or("");
    let override_url = settings
        .ai_catalog
        .overrides
        .get(slug)
        .map(|over| over.base_url.as_str())
        .unwrap_or("");
    normalize_native_chat_url(override_url, default)
}

fn ghost_messages(
    prefix: &str,
    suffix: Option<&str>,
    schema_context: &str,
    avoid: &[String],
) -> Vec<NativeChatMessage> {
    let mut system = String::from(
        "You are a SQL autocomplete engine inside a database client.\n\
         Your task: given the SQL before the cursor and the database schema,\n\
         output ONLY the SQL text that should come next.\n\n\
         RULES:\n\
         1. Output ONLY raw SQL — no markdown, no backticks, no explanations.\n\
         2. Match the existing SQL style (keywords case, indentation).\n\
         3. Use the schema to suggest correct table/column names.\n\
         4. If the statement is already complete, output nothing.\n\
         5. Do NOT repeat what's already typed before or after the cursor.\n",
    );
    if !schema_context.is_empty() {
        system.push_str("\nDatabase schema:\n");
        system.push_str(schema_context);
        if !schema_context.ends_with('\n') {
            system.push('\n');
        }
    }
    if !avoid.is_empty() {
        system.push_str("\nDo not repeat these previous completions:\n");
        for item in avoid {
            system.push_str("- ");
            system.push_str(item);
            system.push('\n');
        }
    }

    let user = match suffix {
        Some(suffix) => {
            format!("Complete between [CURSOR]:\n```sql\n{prefix}[CURSOR]{suffix}\n```")
        }
        None => format!("Complete after [CURSOR]:\n```sql\n{prefix}[CURSOR]\n```"),
    };

    vec![
        NativeChatMessage {
            role: "system".into(),
            content: system,
        },
        NativeChatMessage {
            role: "user".into(),
            content: user,
        },
    ]
}

#[cfg(test)]
mod tests {
    use models::AppUiSettings;
    use services::CompletionToken;

    use super::{ghost_messages, stream_sql_ghost};

    #[test]
    fn stream_sql_ghost_without_provider_completes_immediately() {
        let settings = AppUiSettings::default();
        let mut rx = stream_sql_ghost(&settings, "select ".into(), None, String::new(), &[]);
        let token = rx.try_recv().expect("done token");
        assert!(matches!(token, CompletionToken::Done));
    }

    #[test]
    fn ghost_messages_include_avoid_list() {
        let messages = ghost_messages("select ", None, "-- Table: users\n", &["FROM users".into()]);
        let blob = messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blob.contains("users"));
        assert!(blob.contains("FROM users"));
        assert!(blob.contains("[CURSOR]"));
    }
}
