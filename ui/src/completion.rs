//! SQL completion via `services::complete_sql`.

use models::{
    AppUiSettings,
    builtin_providers,
    is_native_http_ready,
    native_http_has_credentials,
    native_http_provider_enabled,
    normalize_native_chat_url,
    provider_backend,
    provider_offers_complete,
};
use services::{CompleteRequest, complete_sql};
use tokio::sync::mpsc;

pub use services::CompletionToken;

pub(crate) fn complete_request_from_settings(
    settings: &AppUiSettings,
) -> Option<services::CompleteRequest> {
    let active = settings.ai_catalog.active_completion.as_ref()?;
    let provider = active.provider.as_str();
    if !provider_offers_complete(provider, &settings.ai_catalog) {
        return None;
    }
    if !native_http_provider_enabled(&settings.ai_catalog, provider) {
        return None;
    }
    let api_key = settings.lm_api_key(provider);
    if !native_http_has_credentials(provider, &api_key)
        || !is_native_http_ready(provider, &api_key, &settings.ai_catalog)
    {
        return None;
    }
    let backend = provider_backend(provider, &settings.ai_catalog)?;
    Some(CompleteRequest {
        backend,
        base_url: native_base_url(settings, provider),
        api_key,
        model: active.model.clone(),
        prefix: String::new(),
        suffix: None,
        schema_context: String::new(),
    })
}

fn native_base_url(settings: &AppUiSettings, provider: &str) -> String {
    if let Some(custom) = settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
    {
        return normalize_native_chat_url(&custom.base_url, &custom.base_url);
    }
    let default = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
        .map(|spec| spec.default_base_url)
        .unwrap_or("");
    if let Some(over) = settings.ai_catalog.overrides.get(provider)
        && !over.base_url.trim().is_empty()
    {
        return normalize_native_chat_url(&over.base_url, default);
    }
    let vendor = match provider {
        "deepseek" => settings.deepseek.base_url.as_str(),
        "openai" => settings.openai.base_url.as_str(),
        "groq" => settings.groq.base_url.as_str(),
        "openrouter" => settings.openrouter.base_url.as_str(),
        "xai" => settings.xai.base_url.as_str(),
        "mistral" => settings.mistral.base_url.as_str(),
        "ollama" => settings.ollama.base_url.as_str(),
        _ => "",
    };
    normalize_native_chat_url(vendor, default)
}

pub struct CompletionService {
    request: Option<CompleteRequest>,
}

impl CompletionService {
    pub fn new(settings: &AppUiSettings) -> Self {
        Self {
            request: complete_request_from_settings(settings),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.request.is_none()
    }

    /// Stream a completion from the active catalog completion provider.
    pub fn stream_completion(
        &self,
        prefix: String,
        suffix: Option<String>,
        schema_context: String,
    ) -> mpsc::UnboundedReceiver<CompletionToken> {
        let Some(mut req) = self.request.clone() else {
            let (tx, rx) = mpsc::unbounded_channel();
            let _ = tx.send(CompletionToken::Done);
            return rx;
        };
        req.prefix = prefix;
        req.suffix = suffix;
        req.schema_context = schema_context;
        complete_sql(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{ActiveModel, AiProviderOverride, AppUiSettings};

    fn settings_with_completion(provider: &str, model: &str, key: &str) -> AppUiSettings {
        let mut s = AppUiSettings::default();
        s.ai_catalog.active_completion = Some(ActiveModel {
            provider: provider.into(),
            model: model.into(),
        });
        s.ai_catalog.overrides.insert(
            provider.into(),
            AiProviderOverride {
                enabled: true,
                ..Default::default()
            },
        );
        s.set_lm_api_key(provider, key.to_string());
        s
    }

    #[test]
    fn complete_request_uses_active_completion_not_chat() {
        let mut s = settings_with_completion("codestral", "codestral-latest", "sk");
        s.ai_catalog.active = Some(ActiveModel {
            provider: "openai".into(),
            model: "gpt-5.6-sol".into(),
        });
        let req = complete_request_from_settings(&s).expect("req");
        assert_eq!(req.backend, models::AiBackendId::MistralFim);
        assert_eq!(req.model, "codestral-latest");
    }

    #[test]
    fn complete_request_none_without_slot() {
        let s = AppUiSettings::default();
        assert!(complete_request_from_settings(&s).is_none());
    }
}
