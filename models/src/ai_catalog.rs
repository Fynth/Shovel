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

fn default_custom_backend() -> AiBackendId {
    AiBackendId::OpenAiCompat
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomNativeProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<AiModelEntry>,
    #[serde(default = "default_custom_backend")]
    pub backend: AiBackendId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiCatalogSettings {
    pub active: Option<ActiveModel>,
    pub active_completion: Option<ActiveModel>,
    pub overrides: BTreeMap<String, AiProviderOverride>,
    pub custom_native: Vec<CustomNativeProvider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiProviderKind {
    NativeHttp,
    Acp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiProviderGroup {
    Subscription,
    Cloud,
    Local,
    Agent,
}

impl AiProviderGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Subscription => "Subscription",
            Self::Cloud => "Cloud",
            Self::Local => "Local",
            Self::Agent => "Agents",
        }
    }
}

/// Catalog grouping for the agent-panel picker and provider popover.
pub fn provider_group(provider: &str) -> AiProviderGroup {
    if let Some(spec) = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
    {
        return spec.group;
    }
    if provider.starts_with("acp:") {
        AiProviderGroup::Agent
    } else {
        AiProviderGroup::Cloud
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiBackendId {
    OpenAiCompat,
    Ollama,
    MistralFim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiCapabilities {
    pub chat: bool,
    pub complete: bool,
    pub list_models: bool,
}

pub fn backend_capabilities(id: AiBackendId) -> AiCapabilities {
    match id {
        AiBackendId::OpenAiCompat | AiBackendId::Ollama => AiCapabilities {
            chat: true,
            complete: true,
            list_models: true,
        },
        AiBackendId::MistralFim => AiCapabilities {
            chat: false,
            complete: true,
            list_models: false,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinProviderSpec {
    pub slug: &'static str,
    pub label: &'static str,
    pub backend: Option<AiBackendId>,
    pub group: AiProviderGroup,
    pub default_base_url: &'static str,
    pub builtin_models: &'static [(&'static str, &'static str)],
    pub supports_thinking: bool,
}

impl BuiltinProviderSpec {
    pub fn kind(self) -> AiProviderKind {
        if self.backend.is_some() {
            AiProviderKind::NativeHttp
        } else {
            AiProviderKind::Acp
        }
    }

    pub fn supports_model_refresh(self) -> bool {
        self.backend
            .is_some_and(|id| backend_capabilities(id).list_models)
    }
}

pub fn builtin_providers() -> &'static [BuiltinProviderSpec] {
    const DEEPSEEK_MODELS: &[(&str, &str)] = &[
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("deepseek-chat", "DeepSeek Chat"),
        ("deepseek-coder", "DeepSeek Coder"),
    ];
    const OPENAI_MODELS: &[(&str, &str)] = &[
        ("gpt-5.6-sol", "GPT-5.6 Sol"),
        ("gpt-5.6-terra", "GPT-5.6 Terra"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"),
        ("gpt-5.3-codex", "GPT-5.3 Codex"),
        ("gpt-5.5", "GPT-5.5"),
    ];
    const GROQ_MODELS: &[(&str, &str)] = &[
        ("llama-3.3-70b-versatile", "Llama 3.3 70B"),
        ("openai/gpt-oss-120b", "GPT-OSS 120B"),
        ("qwen/qwen3-32b", "Qwen3 32B"),
    ];
    const OPENROUTER_MODELS: &[(&str, &str)] = &[
        ("openai/gpt-5.6-sol", "GPT-5.6 Sol"),
        ("openai/gpt-5.3-codex", "GPT-5.3 Codex"),
        ("anthropic/claude-opus-5", "Claude Opus 5"),
        ("anthropic/claude-sonnet-5", "Claude Sonnet 5"),
        ("google/gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("x-ai/grok-4.6", "Grok 4.6"),
        ("z-ai/glm-5.3", "GLM-5.3"),
        ("minimax/minimax-m3", "MiniMax M3"),
        ("moonshotai/kimi-k3", "Kimi K3"),
        ("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("qwen/qwen3.8-max", "Qwen3.8 Max"),
    ];
    const XAI_MODELS: &[(&str, &str)] = &[
        ("grok-4.6", "Grok 4.6"),
        ("grok-4.5", "Grok 4.5"),
        ("grok-4.3", "Grok 4.3"),
        ("grok-build-0.1", "Grok Build"),
    ];
    const MISTRAL_MODELS: &[(&str, &str)] = &[
        ("mistral-medium-latest", "Mistral Medium"),
        ("mistral-large-latest", "Mistral Large"),
        ("codestral-latest", "Codestral"),
        ("devstral-latest", "Devstral"),
    ];
    const GOOGLE_MODELS: &[(&str, &str)] = &[
        ("gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("gemini-3.6-flash", "Gemini 3.6 Flash"),
        ("gemini-3.5-flash-lite", "Gemini 3.5 Flash-Lite"),
    ];
    const MOONSHOT_MODELS: &[(&str, &str)] = &[
        ("kimi-k3", "Kimi K3"),
        ("kimi-k2.7-code", "Kimi K2.7 Code"),
        ("kimi-k2-turbo-preview", "Kimi K2 Turbo"),
        ("kimi-k2.6", "Kimi K2.6"),
    ];
    const ZHIPU_MODELS: &[(&str, &str)] = &[
        ("glm-5.3", "GLM-5.3"),
        ("glm-5.3-flash", "GLM-5.3 Flash"),
        ("glm-5.2", "GLM-5.2"),
        ("glm-5-turbo", "GLM-5 Turbo"),
    ];
    const ZAI_MODELS: &[(&str, &str)] = &[
        ("glm-5.3", "GLM-5.3"),
        ("glm-5.3-flash", "GLM-5.3 Flash"),
        ("glm-5.2", "GLM-5.2"),
        ("glm-5-turbo", "GLM-5 Turbo"),
    ];
    const ZAI_CODING_MODELS: &[(&str, &str)] = &[
        ("glm-5.3", "GLM-5.3 Coding"),
        ("glm-5.3-flash", "GLM-5.3 Flash Coding"),
        ("glm-5.3-highspeed", "GLM-5.3 Highspeed"),
        ("glm-5.2", "GLM-5.2 Coding"),
        ("glm-5-turbo", "GLM-5 Turbo Coding"),
    ];
    const QWEN_MODELS: &[(&str, &str)] = &[
        ("qwen3.8-max", "Qwen3.8 Max"),
        ("qwen3-coder-plus", "Qwen3 Coder Plus"),
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
        ("MiniMax-M2.7-highspeed", "MiniMax M2.7 Highspeed"),
        ("MiniMax-M2.5", "MiniMax M2.5"),
    ];
    const YI_MODELS: &[(&str, &str)] =
        &[("yi-lightning", "Yi Lightning"), ("yi-large", "Yi Large")];
    const BYTEDANCE_MODELS: &[(&str, &str)] = &[
        ("doubao-seed-2.1-turbo", "Doubao Seed 2.1 Turbo"),
        ("doubao-seed-2.0-code", "Doubao Seed 2.0 Code"),
        ("doubao-seed-1.6", "Doubao Seed 1.6"),
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
    const OPENCODE_GO_MODELS: &[(&str, &str)] = &[
        ("grok-4.6", "Grok 4.6"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"),
        ("glm-5.3", "GLM-5.3"),
        ("glm-5.3-flash", "GLM-5.3 Flash"),
        ("glm-5.2", "GLM-5.2"),
        ("kimi-k3", "Kimi K3"),
        ("kimi-k2.7-code", "Kimi K2.7 Code"),
        ("minimax-m3", "MiniMax M3"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("qwen3.8-max", "Qwen3.8 Max"),
        ("mimo-v2.5-pro", "MiMo V2.5 Pro"),
        ("longcat-2.0", "LongCat 2.0"),
        ("hy3", "Hy3"),
    ];
    const OPENCODE_ZEN_MODELS: &[(&str, &str)] = &[
        ("gpt-5.6-sol", "GPT-5.6 Sol"),
        ("gpt-5.3-codex", "GPT-5.3 Codex"),
        ("glm-5.3", "GLM-5.3"),
        ("kimi-k3", "Kimi K3"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("minimax-m3", "MiniMax M3"),
        ("qwen3.8-max", "Qwen3.8 Max"),
        ("grok-4.6", "Grok 4.6"),
    ];
    const NANOGPT_MODELS: &[(&str, &str)] = &[
        ("openai/gpt-5.6-sol", "GPT-5.6 Sol"),
        ("openai/gpt-5.3-codex", "GPT-5.3 Codex"),
        ("anthropic/claude-opus-4.6", "Claude Opus 4.6"),
        ("anthropic/claude-sonnet-4.6", "Claude Sonnet 4.6"),
        ("google/gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("minimax/minimax-m2.7", "MiniMax M2.7"),
        ("moonshotai/kimi-k3", "Kimi K3"),
        ("zai-org/glm-5.3", "GLM-5.3"),
    ];
    const XIAOMI_MODELS: &[(&str, &str)] = &[
        ("mimo-v2.5-pro", "MiMo V2.5 Pro"),
        ("mimo-v2.5", "MiMo V2.5"),
    ];
    const HUGGINGFACE_MODELS: &[(&str, &str)] = &[
        ("moonshotai/Kimi-K2.5", "Kimi K2.5"),
        ("Qwen/Qwen3.8-27B", "Qwen3.8 27B"),
        ("deepseek-ai/DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
    ];
    const SAMBANOVA_MODELS: &[(&str, &str)] = &[
        ("Meta-Llama-3.3-70B-Instruct", "Llama 3.3 70B"),
        ("DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
    ];
    const HYPERBOLIC_MODELS: &[(&str, &str)] = &[
        ("deepseek-ai/DeepSeek-V4-Pro", "DeepSeek V4 Pro"),
        ("moonshotai/Kimi-K2.5", "Kimi K2.5"),
    ];
    const VENICE_MODELS: &[(&str, &str)] = &[
        ("llama-3.3-70b", "Llama 3.3 70B"),
        ("qwen3-235b", "Qwen3 235B"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
    ];
    const LONGCAT_MODELS: &[(&str, &str)] = &[
        ("longcat-2.0", "LongCat 2.0"),
        ("LongCat-Flash-Chat", "LongCat Flash"),
    ];
    const NOVITA_MODELS: &[(&str, &str)] = &[
        ("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("qwen/qwen3.8-max", "Qwen3.8 Max"),
        ("minimaxai/minimax-m3", "MiniMax M3"),
    ];
    const CODESTRAL_MODELS: &[(&str, &str)] = &[("codestral-latest", "Codestral")];
    const EMPTY_MODELS: &[(&str, &str)] = &[];

    const fn http(
        slug: &'static str,
        label: &'static str,
        backend: AiBackendId,
        group: AiProviderGroup,
        default_base_url: &'static str,
        builtin_models: &'static [(&'static str, &'static str)],
        supports_thinking: bool,
    ) -> BuiltinProviderSpec {
        BuiltinProviderSpec {
            slug,
            label,
            backend: Some(backend),
            group,
            default_base_url,
            builtin_models,
            supports_thinking,
        }
    }

    const fn acp(slug: &'static str, label: &'static str) -> BuiltinProviderSpec {
        BuiltinProviderSpec {
            slug,
            label,
            backend: None,
            group: AiProviderGroup::Agent,
            default_base_url: "",
            builtin_models: EMPTY_MODELS,
            supports_thinking: false,
        }
    }

    const PROVIDERS: &[BuiltinProviderSpec] = &[
        http(
            "deepseek",
            "DeepSeek",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.deepseek.com",
            DEEPSEEK_MODELS,
            true,
        ),
        http(
            "openai",
            "OpenAI",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.openai.com",
            OPENAI_MODELS,
            false,
        ),
        http(
            "groq",
            "Groq",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.groq.com/openai",
            GROQ_MODELS,
            false,
        ),
        http(
            "openrouter",
            "OpenRouter",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://openrouter.ai/api",
            OPENROUTER_MODELS,
            false,
        ),
        http(
            "xai",
            "xAI",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.x.ai",
            XAI_MODELS,
            false,
        ),
        http(
            "mistral",
            "Mistral",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.mistral.ai",
            MISTRAL_MODELS,
            false,
        ),
        http(
            "google",
            "Google Gemini",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://generativelanguage.googleapis.com/v1beta/openai",
            GOOGLE_MODELS,
            false,
        ),
        http(
            "nvidia",
            "NVIDIA NIM",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://integrate.api.nvidia.com/v1",
            NVIDIA_MODELS,
            false,
        ),
        http(
            "ollama",
            "Ollama",
            AiBackendId::Ollama,
            AiProviderGroup::Local,
            "http://localhost:11434",
            EMPTY_MODELS,
            false,
        ),
        // Chinese OpenAI-compatible providers
        http(
            "moonshot",
            "Moonshot (Kimi)",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.moonshot.cn",
            MOONSHOT_MODELS,
            false,
        ),
        http(
            "zhipu",
            "Zhipu (GLM)",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://open.bigmodel.cn/api/paas/v4",
            ZHIPU_MODELS,
            false,
        ),
        http(
            "zai",
            "Z.AI",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.z.ai/api/paas/v4",
            ZAI_MODELS,
            false,
        ),
        http(
            "zai-coding",
            "Z.AI Coding Plan",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Subscription,
            "https://api.z.ai/api/coding/paas/v4",
            ZAI_CODING_MODELS,
            false,
        ),
        http(
            "qwen",
            "Qwen (DashScope)",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://dashscope.aliyuncs.com/compatible-mode",
            QWEN_MODELS,
            false,
        ),
        http(
            "siliconflow",
            "SiliconFlow",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.siliconflow.cn",
            SILICONFLOW_MODELS,
            false,
        ),
        http(
            "minimax",
            "MiniMax",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.minimax.chat/v1",
            MINIMAX_MODELS,
            false,
        ),
        http(
            "yi",
            "01.AI (Yi)",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.lingyiwanwu.com",
            YI_MODELS,
            false,
        ),
        http(
            "bytedance",
            "ByteDance (Doubao)",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://ark.cn-beijing.volces.com/api/v3",
            BYTEDANCE_MODELS,
            false,
        ),
        // Gateways and OpenAI-compatible hosts (Claude/Gemini via OpenRouter)
        http(
            "together",
            "Together",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.together.xyz",
            TOGETHER_MODELS,
            false,
        ),
        http(
            "fireworks",
            "Fireworks",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.fireworks.ai/inference",
            FIREWORKS_MODELS,
            false,
        ),
        http(
            "perplexity",
            "Perplexity",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.perplexity.ai",
            PERPLEXITY_MODELS,
            false,
        ),
        http(
            "cerebras",
            "Cerebras",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.cerebras.ai",
            CEREBRAS_MODELS,
            false,
        ),
        http(
            "deepinfra",
            "DeepInfra",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.deepinfra.com",
            DEEPINFRA_MODELS,
            false,
        ),
        http(
            "opencode-go",
            "OpenCode Go",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Subscription,
            "https://opencode.ai/zen/go",
            OPENCODE_GO_MODELS,
            false,
        ),
        http(
            "opencode-zen",
            "OpenCode Zen",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Subscription,
            "https://opencode.ai/zen",
            OPENCODE_ZEN_MODELS,
            false,
        ),
        http(
            "nanogpt",
            "NanoGPT",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Subscription,
            "https://nano-gpt.com/api",
            NANOGPT_MODELS,
            false,
        ),
        http(
            "xiaomi",
            "Xiaomi MiMo",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.xiaomimimo.com",
            XIAOMI_MODELS,
            false,
        ),
        http(
            "xiaomi-plan",
            "Xiaomi Token Plan",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Subscription,
            "https://token-plan-cn.xiaomimimo.com",
            XIAOMI_MODELS,
            false,
        ),
        http(
            "huggingface",
            "Hugging Face",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://router.huggingface.co",
            HUGGINGFACE_MODELS,
            false,
        ),
        http(
            "sambanova",
            "SambaNova",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.sambanova.ai",
            SAMBANOVA_MODELS,
            false,
        ),
        http(
            "hyperbolic",
            "Hyperbolic",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.hyperbolic.xyz",
            HYPERBOLIC_MODELS,
            false,
        ),
        http(
            "venice",
            "Venice",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.venice.ai/api",
            VENICE_MODELS,
            false,
        ),
        http(
            "longcat",
            "LongCat",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.longcat.chat/openai",
            LONGCAT_MODELS,
            false,
        ),
        http(
            "novita",
            "Novita",
            AiBackendId::OpenAiCompat,
            AiProviderGroup::Cloud,
            "https://api.novita.ai/v3/openai",
            NOVITA_MODELS,
            false,
        ),
        http(
            "codestral",
            "Codestral",
            AiBackendId::MistralFim,
            AiProviderGroup::Cloud,
            "https://codestral.mistral.ai",
            CODESTRAL_MODELS,
            false,
        ),
        acp("acp:opencode", "OpenCode"),
        acp("acp:codex", "Codex"),
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
        return Some(spec.kind());
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

pub fn provider_backend(provider: &str, catalog: &AiCatalogSettings) -> Option<AiBackendId> {
    if let Some(spec) = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
    {
        return spec.backend;
    }
    if let Some(custom) = catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
    {
        return Some(custom.backend);
    }
    if provider.starts_with("custom:") {
        return Some(AiBackendId::OpenAiCompat);
    }
    None
}

pub fn provider_offers_chat(provider: &str, catalog: &AiCatalogSettings) -> bool {
    provider_backend(provider, catalog).is_some_and(|id| backend_capabilities(id).chat)
}

pub fn provider_offers_complete(provider: &str, catalog: &AiCatalogSettings) -> bool {
    provider_backend(provider, catalog).is_some_and(|id| backend_capabilities(id).complete)
}

/// Remove a custom native provider. Clears `active` / `active_completion` when they point at it.
pub fn delete_custom_provider(cat: &mut AiCatalogSettings, id: &str) {
    cat.custom_native.retain(|provider| provider.id != id);
    if cat
        .active
        .as_ref()
        .is_some_and(|active| active.provider == id)
    {
        cat.active = None;
    }
    if cat
        .active_completion
        .as_ref()
        .is_some_and(|active| active.provider == id)
    {
        cat.active_completion = None;
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
            active_completion: None,
            overrides: BTreeMap::new(),
            custom_native: vec![CustomNativeProvider {
                id: "custom:1".into(),
                name: "Mine".into(),
                base_url: "http://localhost:8080".into(),
                models: vec![AiModelEntry {
                    id: "m".into(),
                    label: String::new(),
                }],
                backend: AiBackendId::OpenAiCompat,
            }],
        };
        delete_custom_provider(&mut cat, "custom:1");
        assert!(cat.custom_native.is_empty());
        assert!(cat.active.is_none());
    }

    #[test]
    fn default_custom_backend_is_openai_compat() {
        let json = r#"{"id":"custom:1","name":"Mine","base_url":"http://localhost","models":[]}"#;
        let custom: CustomNativeProvider = serde_json::from_str(json).unwrap();
        assert_eq!(custom.backend, AiBackendId::OpenAiCompat);
    }

    #[test]
    fn provider_backend_reads_spec_and_custom() {
        let mut cat = AiCatalogSettings::default();
        cat.custom_native.push(CustomNativeProvider {
            id: "custom:1".into(),
            name: "Mine".into(),
            base_url: "http://localhost".into(),
            models: vec![],
            backend: AiBackendId::OpenAiCompat,
        });
        assert_eq!(
            provider_backend("deepseek", &cat),
            Some(AiBackendId::OpenAiCompat)
        );
        assert_eq!(provider_backend("ollama", &cat), Some(AiBackendId::Ollama));
        assert_eq!(
            provider_backend("codestral", &cat),
            Some(AiBackendId::MistralFim)
        );
        assert_eq!(
            provider_backend("custom:1", &cat),
            Some(AiBackendId::OpenAiCompat)
        );
        assert_eq!(provider_backend("acp:codex", &cat), None);
        assert!(provider_offers_chat("openai", &cat));
        assert!(!provider_offers_chat("codestral", &cat));
        assert!(provider_offers_complete("codestral", &cat));
        assert!(!provider_offers_complete("acp:codex", &cat));
    }

    #[test]
    fn delete_custom_clears_completion_slot() {
        let mut cat = AiCatalogSettings {
            active: Some(ActiveModel {
                provider: "openai".into(),
                model: "m".into(),
            }),
            active_completion: Some(ActiveModel {
                provider: "custom:1".into(),
                model: "m".into(),
            }),
            overrides: BTreeMap::new(),
            custom_native: vec![CustomNativeProvider {
                id: "custom:1".into(),
                name: "Mine".into(),
                base_url: "http://localhost".into(),
                models: vec![],
                backend: AiBackendId::OpenAiCompat,
            }],
        };
        delete_custom_provider(&mut cat, "custom:1");
        assert!(cat.custom_native.is_empty());
        assert_eq!(cat.active.as_ref().unwrap().provider, "openai");
        assert!(cat.active_completion.is_none());
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
            "zai",
            "zai-coding",
            "minimax",
            "bytedance",
            "opencode-go",
            "opencode-zen",
            "nanogpt",
            "xiaomi",
            "xiaomi-plan",
            "huggingface",
            "venice",
            "longcat",
            "novita",
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
        assert!(
            xai.builtin_models
                .iter()
                .any(|(id, _)| *id == "grok-build-0.1")
        );
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
        let zai = builtin_providers()
            .iter()
            .find(|p| p.slug == "zai")
            .unwrap();
        assert!(zai.builtin_models.iter().any(|(id, _)| *id == "glm-5.3"));
        let zai_coding = builtin_providers()
            .iter()
            .find(|p| p.slug == "zai-coding")
            .unwrap();
        assert_eq!(
            zai_coding.default_base_url,
            "https://api.z.ai/api/coding/paas/v4"
        );
        let minimax = builtin_providers()
            .iter()
            .find(|p| p.slug == "minimax")
            .unwrap();
        assert!(
            minimax
                .builtin_models
                .iter()
                .any(|(id, _)| *id == "MiniMax-M3")
        );
        let moonshot = builtin_providers()
            .iter()
            .find(|p| p.slug == "moonshot")
            .unwrap();
        assert!(
            moonshot
                .builtin_models
                .iter()
                .any(|(id, _)| *id == "kimi-k2.7-code")
        );
        let qwen = builtin_providers()
            .iter()
            .find(|p| p.slug == "qwen")
            .unwrap();
        assert!(
            qwen.builtin_models
                .iter()
                .any(|(id, _)| *id == "qwen3-coder-plus")
        );
        let go = builtin_providers()
            .iter()
            .find(|p| p.slug == "opencode-go")
            .unwrap();
        assert_eq!(go.default_base_url, "https://opencode.ai/zen/go");
        assert!(go.builtin_models.iter().any(|(id, _)| *id == "grok-4.6"));
        let nanogpt = builtin_providers()
            .iter()
            .find(|p| p.slug == "nanogpt")
            .unwrap();
        assert_eq!(nanogpt.default_base_url, "https://nano-gpt.com/api");
        assert!(
            nanogpt
                .builtin_models
                .iter()
                .any(|(id, _)| *id == "openai/gpt-5.6-sol")
        );
    }

    #[test]
    fn backend_capabilities_match_protocol() {
        let openai = backend_capabilities(AiBackendId::OpenAiCompat);
        assert!(openai.chat && openai.complete && openai.list_models);
        let ollama = backend_capabilities(AiBackendId::Ollama);
        assert!(ollama.chat && ollama.complete && ollama.list_models);
        let fim = backend_capabilities(AiBackendId::MistralFim);
        assert!(!fim.chat && fim.complete && !fim.list_models);
    }

    #[test]
    fn codestral_is_mistral_fim_complete_only() {
        let spec = builtin_providers()
            .iter()
            .find(|p| p.slug == "codestral")
            .expect("codestral");
        assert_eq!(spec.backend, Some(AiBackendId::MistralFim));
        assert_eq!(spec.group, AiProviderGroup::Cloud);
        assert_eq!(spec.default_base_url, "https://codestral.mistral.ai");
        assert!(!spec.supports_thinking);
        assert_eq!(spec.kind(), AiProviderKind::NativeHttp);
        assert!(!spec.supports_model_refresh());
        assert!(
            spec.builtin_models
                .iter()
                .any(|(id, _)| *id == "codestral-latest")
        );
    }

    #[test]
    fn spec_fields_replace_slug_tables() {
        let deepseek = builtin_providers()
            .iter()
            .find(|p| p.slug == "deepseek")
            .unwrap();
        assert_eq!(deepseek.backend, Some(AiBackendId::OpenAiCompat));
        assert!(deepseek.supports_thinking);
        assert_eq!(deepseek.group, AiProviderGroup::Cloud);

        let ollama = builtin_providers()
            .iter()
            .find(|p| p.slug == "ollama")
            .unwrap();
        assert_eq!(ollama.backend, Some(AiBackendId::Ollama));
        assert_eq!(ollama.group, AiProviderGroup::Local);
        assert!(!ollama.supports_thinking);

        let go = builtin_providers()
            .iter()
            .find(|p| p.slug == "opencode-go")
            .unwrap();
        assert_eq!(go.group, AiProviderGroup::Subscription);
        assert_eq!(go.backend, Some(AiBackendId::OpenAiCompat));

        let acp = builtin_providers()
            .iter()
            .find(|p| p.slug == "acp:codex")
            .unwrap();
        assert_eq!(acp.backend, None);
        assert_eq!(acp.kind(), AiProviderKind::Acp);
        assert_eq!(acp.group, AiProviderGroup::Agent);
        assert!(!acp.supports_model_refresh());
    }

    #[test]
    fn provider_group_reads_spec_not_slug_table() {
        assert_eq!(provider_group("opencode-go"), AiProviderGroup::Subscription);
        assert_eq!(provider_group("nanogpt"), AiProviderGroup::Subscription);
        assert_eq!(provider_group("zai-coding"), AiProviderGroup::Subscription);
        assert_eq!(provider_group("xiaomi-plan"), AiProviderGroup::Subscription);
        assert_eq!(provider_group("openai"), AiProviderGroup::Cloud);
        assert_eq!(provider_group("ollama"), AiProviderGroup::Local);
        assert_eq!(provider_group("acp:codex"), AiProviderGroup::Agent);
        assert_eq!(provider_group("acp:unknown"), AiProviderGroup::Agent);
        assert_eq!(provider_group("custom:1"), AiProviderGroup::Cloud);
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
