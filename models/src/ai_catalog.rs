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

impl AiModelEntry {
    pub fn display_label(&self) -> &str {
        if self.label.trim().is_empty() {
            &self.id
        } else {
            &self.label
        }
    }
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
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("deepseek-chat", "DeepSeek Chat"),
    ];
    const OPENAI_MODELS: &[(&str, &str)] = &[
        ("gpt-5.6-sol", "GPT-5.6 Sol"),
        ("gpt-5.6-terra", "GPT-5.6 Terra"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"),
    ];
    const GROQ_MODELS: &[(&str, &str)] = &[
        ("llama-3.3-70b-versatile", "Llama 3.3 70B"),
        ("openai/gpt-oss-120b", "GPT-OSS 120B"),
        ("qwen/qwen3-32b", "Qwen3 32B"),
    ];
    const OPENROUTER_MODELS: &[(&str, &str)] = &[
        ("openai/gpt-5.6-sol", "GPT-5.6 Sol"),
        ("anthropic/claude-opus-5", "Claude Opus 5"),
        ("google/gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("x-ai/grok-4.6", "Grok 4.6"),
        ("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro"),
    ];
    const XAI_MODELS: &[(&str, &str)] = &[
        ("grok-4.6", "Grok 4.6"),
        ("grok-4.5", "Grok 4.5"),
        ("grok-4.3", "Grok 4.3"),
    ];
    const MISTRAL_MODELS: &[(&str, &str)] = &[
        ("mistral-medium-latest", "Mistral Medium"),
        ("mistral-large-latest", "Mistral Large"),
        ("codestral-latest", "Codestral"),
    ];
    const GOOGLE_MODELS: &[(&str, &str)] = &[
        ("gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("gemini-3.6-flash", "Gemini 3.6 Flash"),
        ("gemini-3.5-flash-lite", "Gemini 3.5 Flash-Lite"),
    ];
    const MOONSHOT_MODELS: &[(&str, &str)] = &[
        ("kimi-k3", "Kimi K3"),
        ("kimi-k2-turbo-preview", "Kimi K2 Turbo"),
        ("kimi-k2.6", "Kimi K2.6"),
    ];
    const ZHIPU_MODELS: &[(&str, &str)] = &[
        ("glm-5.3", "GLM-5.3"),
        ("glm-5.3-flash", "GLM-5.3 Flash"),
        ("glm-4.5", "GLM-4.5"),
    ];
    const QWEN_MODELS: &[(&str, &str)] = &[
        ("qwen3.8-max", "Qwen3.8 Max"),
        ("qwen-plus", "Qwen Plus"),
        ("qwen-flash", "Qwen Flash"),
    ];
    const SILICONFLOW_MODELS: &[(&str, &str)] = &[
        ("deepseek-ai/DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
        ("Qwen/Qwen3.8-27B", "Qwen3.8 27B"),
        ("moonshotai/Kimi-K2-Instruct", "Kimi K2"),
    ];
    const MINIMAX_MODELS: &[(&str, &str)] = &[
        ("MiniMax-M3", "MiniMax M3"),
        ("MiniMax-M2.7", "MiniMax M2.7"),
    ];
    const YI_MODELS: &[(&str, &str)] =
        &[("yi-lightning", "Yi Lightning"), ("yi-large", "Yi Large")];
    const BYTEDANCE_MODELS: &[(&str, &str)] = &[
        ("doubao-seed-1.6", "Doubao Seed 1.6"),
        ("doubao-1.5-pro-32k", "Doubao 1.5 Pro"),
    ];
    const TOGETHER_MODELS: &[(&str, &str)] = &[
        ("deepseek-ai/DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
        ("Qwen/Qwen3.8-27B-Instruct", "Qwen3.8 27B"),
        ("meta-llama/Llama-3.3-70B-Instruct-Turbo", "Llama 3.3 70B"),
    ];
    const FIREWORKS_MODELS: &[(&str, &str)] = &[
        (
            "accounts/fireworks/models/deepseek-v4-pro",
            "DeepSeek V4 Pro",
        ),
        (
            "accounts/fireworks/models/deepseek-v4-flash",
            "DeepSeek V4 Flash",
        ),
    ];
    const PERPLEXITY_MODELS: &[(&str, &str)] = &[
        ("sonar-pro", "Sonar Pro"),
        ("sonar-reasoning-pro", "Sonar Reasoning Pro"),
        ("sonar", "Sonar"),
    ];
    const CEREBRAS_MODELS: &[(&str, &str)] = &[
        ("llama-3.3-70b", "Llama 3.3 70B"),
        ("qwen-3-32b", "Qwen3 32B"),
    ];
    const DEEPINFRA_MODELS: &[(&str, &str)] = &[
        ("deepseek-ai/DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
        ("meta-llama/Llama-3.3-70B-Instruct", "Llama 3.3 70B"),
    ];
    const NVIDIA_MODELS: &[(&str, &str)] = &[
        ("nvidia/nemotron-3.5-lightning", "Nemotron 3.5 Lightning"),
        ("meta/llama-3.3-70b-instruct", "Llama 3.3 70B"),
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
            slug: "google",
            label: "Google Gemini",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            builtin_models: GOOGLE_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "nvidia",
            label: "NVIDIA NIM",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://integrate.api.nvidia.com/v1",
            builtin_models: NVIDIA_MODELS,
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
        // Chinese OpenAI-compatible providers
        BuiltinProviderSpec {
            slug: "moonshot",
            label: "Moonshot (Kimi)",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.moonshot.cn",
            builtin_models: MOONSHOT_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "zhipu",
            label: "Zhipu (GLM)",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://open.bigmodel.cn/api/paas/v4",
            builtin_models: ZHIPU_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "qwen",
            label: "Qwen (DashScope)",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://dashscope.aliyuncs.com/compatible-mode",
            builtin_models: QWEN_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "siliconflow",
            label: "SiliconFlow",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.siliconflow.cn",
            builtin_models: SILICONFLOW_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "minimax",
            label: "MiniMax",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.minimax.chat/v1",
            builtin_models: MINIMAX_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "yi",
            label: "01.AI (Yi)",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.lingyiwanwu.com",
            builtin_models: YI_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "bytedance",
            label: "ByteDance (Doubao)",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
            builtin_models: BYTEDANCE_MODELS,
            supports_model_refresh: true,
        },
        // Gateways and OpenAI-compatible hosts (Claude/Gemini via OpenRouter)
        BuiltinProviderSpec {
            slug: "together",
            label: "Together",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.together.xyz",
            builtin_models: TOGETHER_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "fireworks",
            label: "Fireworks",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.fireworks.ai/inference",
            builtin_models: FIREWORKS_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "perplexity",
            label: "Perplexity",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.perplexity.ai",
            builtin_models: PERPLEXITY_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "cerebras",
            label: "Cerebras",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.cerebras.ai",
            builtin_models: CEREBRAS_MODELS,
            supports_model_refresh: true,
        },
        BuiltinProviderSpec {
            slug: "deepinfra",
            label: "DeepInfra",
            kind: AiProviderKind::NativeHttp,
            default_base_url: "https://api.deepinfra.com",
            builtin_models: DEEPINFRA_MODELS,
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

/// Catalog kind for a provider id. Unknown ids are `None` unless they use `custom:`.
pub fn provider_kind(provider: &str) -> Option<AiProviderKind> {
    if let Some(spec) = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
    {
        return Some(spec.kind);
    }
    if provider.starts_with("custom:") {
        return Some(AiProviderKind::NativeHttp);
    }
    if provider.starts_with("acp:") {
        return Some(AiProviderKind::Acp);
    }
    None
}

/// Credential check without the catalog `enabled` flag.
///
/// Ollama allows an empty key; other NativeHttp providers need a key.
/// Acp is never ready this way. Used by the leftover Connect form, which
/// enables the provider as it connects.
pub fn native_http_has_credentials(provider: &str, api_key: &str) -> bool {
    match provider_kind(provider) {
        Some(AiProviderKind::NativeHttp) => provider == "ollama" || !api_key.trim().is_empty(),
        Some(AiProviderKind::Acp) | None => false,
    }
}

/// Whether a NativeHttp provider is selectable / auto-connectable.
///
/// Builtins require `overrides[slug].enabled`. Custom `custom:*` ids have no
/// enabled flag and are treated as enabled. Acp ids are never enabled here.
pub fn native_http_provider_enabled(catalog: &AiCatalogSettings, provider: &str) -> bool {
    if provider.starts_with("custom:") {
        return provider_kind(provider) == Some(AiProviderKind::NativeHttp);
    }
    catalog
        .overrides
        .get(provider)
        .is_some_and(|over| over.enabled)
}

/// Native HTTP is ready without an ACP child: enabled in the catalog and
/// credentials present. Disabled builtins are not selectable or auto-connected.
pub fn is_native_http_ready(provider: &str, api_key: &str, catalog: &AiCatalogSettings) -> bool {
    native_http_provider_enabled(catalog, provider)
        && native_http_has_credentials(provider, api_key)
}

/// True when switching `from` → `to` must tear down or launch an ACP child.
/// NativeHttp ↔ NativeHttp is a persist-only hot-swap.
pub fn needs_acp_reconnect(from: &str, to: &str) -> bool {
    let from_kind = provider_kind(from);
    let to_kind = provider_kind(to);
    from_kind != to_kind
        || ((from_kind == Some(AiProviderKind::Acp) || to_kind == Some(AiProviderKind::Acp))
            && from != to)
}

/// Remove a custom native provider. If it was the active model, clear `active`.
pub fn delete_custom_provider(cat: &mut AiCatalogSettings, id: &str) {
    cat.custom_native.retain(|provider| provider.id != id);
    if cat
        .active
        .as_ref()
        .is_some_and(|active| active.provider == id)
    {
        cat.active = None;
    }
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
    use std::collections::BTreeMap;

    #[test]
    fn delete_custom_resets_active_when_it_was_selected() {
        let mut cat = AiCatalogSettings {
            active: Some(ActiveModel {
                provider: "custom:1".into(),
                model: "m".into(),
            }),
            overrides: BTreeMap::new(),
            custom_native: vec![CustomNativeProvider {
                id: "custom:1".into(),
                name: "Mine".into(),
                base_url: "http://localhost:8080".into(),
                models: vec![AiModelEntry {
                    id: "m".into(),
                    label: String::new(),
                }],
            }],
        };
        delete_custom_provider(&mut cat, "custom:1");
        assert!(cat.custom_native.is_empty());
        assert!(cat.active.is_none());
    }

    #[test]
    fn active_model_label_falls_back_to_id() {
        let e = AiModelEntry {
            id: "gpt-4o".into(),
            label: String::new(),
        };
        assert_eq!(e.display_label(), "gpt-4o");
        let e = AiModelEntry {
            id: "gpt-4o".into(),
            label: "GPT-4o".into(),
        };
        assert_eq!(e.display_label(), "GPT-4o");
    }

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

    fn catalog_with_enabled(slugs: &[&str]) -> AiCatalogSettings {
        let mut cat = AiCatalogSettings::default();
        for slug in slugs {
            cat.overrides.insert(
                (*slug).to_string(),
                AiProviderOverride {
                    enabled: true,
                    ..Default::default()
                },
            );
        }
        cat
    }

    #[test]
    fn native_http_is_ready_without_acp_child() {
        let cat = catalog_with_enabled(&["openai", "ollama"]);
        assert!(is_native_http_ready("openai", "sk-test", &cat));
        assert!(is_native_http_ready("ollama", "", &cat));
        assert!(!is_native_http_ready("openai", "", &cat));
        assert!(!is_native_http_ready("acp:opencode", "sk", &cat));
        assert!(native_http_has_credentials("openai", "sk-test"));
        assert!(!native_http_has_credentials("openai", ""));
    }

    #[test]
    fn custom_native_http_requires_non_empty_key() {
        let cat = AiCatalogSettings::default();
        assert!(is_native_http_ready("custom:abc", "sk", &cat));
        assert!(!is_native_http_ready("custom:abc", "", &cat));
        assert!(!is_native_http_ready("unknown", "sk", &cat));
    }

    #[test]
    fn builtin_catalog_has_current_us_and_cn_models() {
        let slugs: Vec<_> = builtin_providers().iter().map(|p| p.slug).collect();
        for slug in [
            "openai",
            "xai",
            "google",
            "deepseek",
            "qwen",
            "moonshot",
            "zhipu",
            "minimax",
            "bytedance",
        ] {
            assert!(slugs.contains(&slug), "missing provider {slug}");
        }
        let openai = builtin_providers()
            .iter()
            .find(|p| p.slug == "openai")
            .unwrap();
        assert!(
            openai
                .builtin_models
                .iter()
                .any(|(id, _)| *id == "gpt-5.6-sol")
        );
        assert!(!openai.builtin_models.iter().any(|(id, _)| *id == "gpt-4o"));
        let xai = builtin_providers()
            .iter()
            .find(|p| p.slug == "xai")
            .unwrap();
        assert!(xai.builtin_models.iter().any(|(id, _)| *id == "grok-4.6"));
        let openrouter = builtin_providers()
            .iter()
            .find(|p| p.slug == "openrouter")
            .unwrap();
        assert!(
            openrouter
                .builtin_models
                .iter()
                .any(|(id, _)| *id == "anthropic/claude-opus-5")
        );
    }

    #[test]
    fn acp_prefixed_ids_are_acp_kind() {
        let cat = AiCatalogSettings::default();
        assert_eq!(provider_kind("acp:custom"), Some(AiProviderKind::Acp));
        assert_eq!(provider_kind("acp:opencode"), Some(AiProviderKind::Acp));
        assert!(!is_native_http_ready("acp:custom", "sk", &cat));
    }

    #[test]
    fn disabled_native_http_is_not_ready() {
        let cat = AiCatalogSettings::default();
        assert!(!native_http_provider_enabled(&cat, "openai"));
        assert!(!is_native_http_ready("openai", "sk-test", &cat));

        let cat = catalog_with_enabled(&["openai"]);
        assert!(native_http_provider_enabled(&cat, "openai"));
        assert!(is_native_http_ready("openai", "sk-test", &cat));

        let mut cat = catalog_with_enabled(&["openai"]);
        cat.overrides.get_mut("openai").expect("openai").enabled = false;
        assert!(!is_native_http_ready("openai", "sk-test", &cat));
        assert!(native_http_has_credentials("openai", "sk-test"));
    }

    #[test]
    fn native_to_native_does_not_need_reconnect() {
        assert!(!needs_acp_reconnect("openai", "deepseek"));
        assert!(needs_acp_reconnect("openai", "acp:opencode"));
        assert!(needs_acp_reconnect("acp:opencode", "openai"));
        assert!(!needs_acp_reconnect("openai", "openai"));
    }
}
