use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveModel {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiModelEntry {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiProviderOverride {
    pub enabled: bool,
    pub base_url: String,
    pub extra_models: Vec<AiModelEntry>,
    pub hidden_builtin_ids: Vec<String>,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
}

impl Default for AiProviderOverride {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            extra_models: Vec::new(),
            hidden_builtin_ids: Vec::new(),
            thinking_enabled: false,
            reasoning_effort: "medium".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomNativeProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<AiModelEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiCatalogSettings {
    pub active: Option<ActiveModel>,
    pub overrides: BTreeMap<String, AiProviderOverride>,
    pub custom_native: Vec<CustomNativeProvider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiProviderKind {
    NativeHttp,
    Acp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinProviderSpec {
    pub slug: &'static str,
    pub label: &'static str,
    pub kind: AiProviderKind,
    pub default_base_url: &'static str,
    pub builtin_models: &'static [(&'static str, &'static str)],
    pub supports_model_refresh: bool,
}

pub fn builtin_providers() -> &'static [BuiltinProviderSpec] {
    const DEEPSEEK_MODELS: &[(&str, &str)] = &[
        ("deepseek-chat", ""),
        ("deepseek-v4-pro", ""),
        ("deepseek-v4-flash", ""),
    ];
    const OPENAI_MODELS: &[(&str, &str)] = &[
        ("gpt-4.1", ""),
        ("gpt-4.1-mini", ""),
        ("gpt-4o", ""),
        ("gpt-4o-mini", ""),
        ("o4-mini", ""),
    ];
    const GROQ_MODELS: &[(&str, &str)] = &[
        ("llama-3.3-70b-versatile", ""),
        ("openai/gpt-oss-120b", ""),
        ("qwen/qwen3-32b", ""),
    ];
    const OPENROUTER_MODELS: &[(&str, &str)] = &[
        ("openai/gpt-4o", ""),
        ("anthropic/claude-sonnet-4", ""),
        ("google/gemini-2.5-pro", ""),
    ];
    const XAI_MODELS: &[(&str, &str)] = &[("grok-4", ""), ("grok-3", ""), ("grok-3-mini", "")];
    const MISTRAL_MODELS: &[(&str, &str)] = &[
        ("mistral-large-latest", ""),
        ("codestral-latest", ""),
        ("mistral-small-latest", ""),
    ];
    const EMPTY_MODELS: &[(&str, &str)] = &[];

    const PROVIDERS: &[BuiltinProviderSpec] = &[
        BuiltinProviderSpec {
            slug: "deepseek",
            label: "DeepSeek",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.deepseek.com",
            builtin_models: DEEPSEEK_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "openai",
            label: "OpenAI",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.openai.com",
            builtin_models: OPENAI_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "groq",
            label: "Groq",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.groq.com/openai",
            builtin_models: GROQ_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "openrouter",
            label: "OpenRouter",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://openrouter.ai/api",
            builtin_models: OPENROUTER_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "xai",
            label: "xAI",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.x.ai",
            builtin_models: XAI_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "mistral",
            label: "Mistral",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.mistral.ai",
            builtin_models: MISTRAL_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "ollama",
            label: "Ollama",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "http://localhost:11434",
            builtin_models: EMPTY_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "acp:opencode",
            label: "OpenCode",
            kind: AiProviderKind::Acp,
            default_base_url: "",
            builtin_models: EMPTY_MODELS,
            supports_model_refresh: false,
        },
        BuiltinProviderSpec {
            slug: "acp:codex",
            label: "Codex",
            kind: AiProviderKind::Acp,
            default_base_url: "",
            builtin_models: EMPTY_MODELS,
            supports_model_refresh: false,
        },
    ];

    PROVIDERS
}

/// Merge builtin models with extras: drop hidden builtins, then append extras whose ids are new.
pub fn resolve_picker_models(
    builtin: &[AiModelEntry],
    extra: &[AiModelEntry],
    hidden: &[String],
) -> Vec<AiModelEntry> {
    let mut out = Vec::with_capacity(builtin.len() + extra.len());
    for model in builtin {
        if hidden.iter().any(|id| id == &model.id) {
            continue;
        }
        out.push(model.clone());
    }
    for model in extra {
        if out.iter().any(|existing| existing.id == model.id) {
            continue;
        }
        out.push(model.clone());
    }
    out
}

/// Trim `base`, fall back to `default_base` when empty, strip a trailing `/`.
pub fn normalize_native_chat_url(base: &str, default_base: &str) -> String {
    let trimmed = base.trim();
    let chosen = if trimmed.is_empty() {
        default_base
    } else {
        trimmed
    };
    chosen.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_picker_models_hides_builtins_and_appends_extra_without_dupes() {
        let builtin = vec![
            AiModelEntry {
                id: "gpt-4o".into(),
                label: String::new(),
            },
            AiModelEntry {
                id: "gpt-4o-mini".into(),
                label: String::new(),
            },
        ];
        let extra = vec![
            AiModelEntry {
                id: "gpt-4o".into(),
                label: "dup".into(),
            },
            AiModelEntry {
                id: "my-ft".into(),
                label: "Fine-tune".into(),
            },
        ];
        let hidden = vec!["gpt-4o-mini".into()];
        let got = resolve_picker_models(&builtin, &extra, &hidden);
        let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-4o", "my-ft"]);
    }

    #[test]
    fn normalize_native_chat_url_strips_slash_and_does_not_double_v1() {
        assert_eq!(
            normalize_native_chat_url("https://api.openai.com/", "https://api.openai.com"),
            "https://api.openai.com"
        );
        assert_eq!(
            normalize_native_chat_url("https://api.openai.com/v1", "https://api.openai.com"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_native_chat_url("", "https://api.deepseek.com"),
            "https://api.deepseek.com"
        );
    }
}
