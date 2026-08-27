# Modular AI Backends Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace slug `if`s and UI-owned completion HTTP with a const vendor catalog plus three protocol backends, and let chat and SQL autocomplete share that catalog through separate `active` / `active_completion` slots.

**Architecture:** Vendors stay data in `models` (`backend`, `group`, `supports_thinking`). Protocol code lives in `acp-core/src/backends/` (`OpenAiCompat`, `Ollama`, `MistralFim`). `services` re-exports `native_chat_prompt`, `complete_sql`, and `refresh_provider_models(backend, …)`. `ui` calls those facades only.

**Tech Stack:** Rust nightly (workspace pin), Dioxus 0.7, serde, reqwest, tokio, keyring.

**Spec:** `docs/superpowers/specs/2026-08-27-modular-ai-backends-design.md`

## Global Constraints

- Dioxus 0.7 only (`use_signal`, `use_effect`, `#[component]`). No `cx` / `Scope` / `use_state`.
- Never hold a signal read/write across `.await`.
- `ui` may import `models` and `services` only. After Task 10, `ui` must not import `reqwest` and must not contain vendor `https://` URLs in `completion.rs`.
- Runtime dispatches on `AiBackendId`, never on vendor slug (`"ollama"`, `"deepseek"`).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- API keys stay in keyring `shovel.lm.<provider_id>`; JSON never serializes secrets.
- Do not add Anthropic / Gemini-native / Bedrock backends.
- Do not put ACP behind `AiBackend`. Autocomplete never uses ACP.
- Do not implement editor menu / ghost-text layout / variant cycling from the SQL completion spec.
- Do not delete legacy `DeepSeekSettings` / `CodeStralSettings` structs.
- Do not add a new workspace crate.
- Each task must leave the workspace compiling.

## File structure

- Modify: `models/src/ai_catalog.rs` — `AiBackendId`, capabilities, spec fields, `active_completion`, custom `backend`, helpers
- Modify: `models/src/settings.rs` — migrate `active_completion` from codestral/deepseek
- Modify: `models/src/settings_roundtrip.rs` — only if a roundtrip assertion needs `active_completion`
- Modify: `services/src/app.rs` — `spec.kind()`; add `("codestral", "shovel.codestral")` to `LEGACY_LM_KEYRING`
- Create: `acp-core/src/backends/mod.rs`
- Create: `acp-core/src/backends/openai.rs`
- Create: `acp-core/src/backends/ollama.rs`
- Create: `acp-core/src/backends/mistral_fim.rs`
- Create: `acp-core/src/native_complete.rs`
- Modify: `acp-core/src/native_chat.rs` — `NativeChatRequest` + dispatch through trait
- Modify: `acp-core/src/native_runtime.rs` — refresh by `AiBackendId`
- Modify: `acp-core/src/lib.rs` — `mod backends; mod native_complete;` re-exports
- Modify: `acp/src/lib.rs` — re-export `complete_sql`, `CompletionToken`, `AiBackendId` if needed by services
- Modify: `services/src/lib.rs` — re-export `complete_sql`, `CompletionToken`
- Modify: `services/tests/facade_smoke.rs` — `complete_sql`
- Modify: `ui/src/screens/workspace/components/agent_panel/requests.rs` — `backend` + `supports_thinking`
- Modify: `ui/src/screens/workspace/components/agent_panel/catalog.rs` — `spec.kind()`, `spec.group`, refresh by backend, chat rows require `chat`
- Modify: `ui/src/layout/settings_modal/sections.rs` — `spec.kind()`, refresh by backend, SQL autocomplete selects
- Modify: `ui/src/app_state/mod.rs` — `set_active_completion`
- Modify: `ui/src/completion.rs` — thin wrapper, no `reqwest`
- Modify: `ui/Cargo.toml` — drop `reqwest` when unused
- Do not edit `acp-core/src/deepseek.rs` / `ollama.rs` CLI agents

---

### Task 1: Backend id, capabilities, spec fields, Codestral row

**Files:**
- Modify: `models/src/ai_catalog.rs`
- Modify: `services/src/app.rs` (field `spec.kind` → method)
- Modify: `ui/src/screens/workspace/components/agent_panel/catalog.rs`
- Modify: `ui/src/layout/settings_modal/sections.rs`

**Interfaces:**
- Consumes: existing `BuiltinProviderSpec`, `AiProviderKind`, `AiProviderGroup`, `builtin_providers()`.
- Produces:
  - `pub enum AiBackendId { OpenAiCompat, Ollama, MistralFim }` with `Serialize, Deserialize, Copy`, `#[serde(rename_all = "snake_case")]`
  - `pub struct AiCapabilities { pub chat: bool, pub complete: bool, pub list_models: bool }`
  - `pub fn backend_capabilities(id: AiBackendId) -> AiCapabilities`
  - `BuiltinProviderSpec { slug, label, backend: Option<AiBackendId>, group: AiProviderGroup, default_base_url, builtin_models, supports_thinking: bool }`
  - `impl BuiltinProviderSpec { pub fn kind(self) -> AiProviderKind; pub fn supports_model_refresh(self) -> bool }`
  - `provider_kind` uses `spec.kind()` / `spec.backend`
  - `provider_group(provider)` looks up `spec.group`; unknown `acp:*` → `Agent`; else `Cloud`
  - builtin `codestral` with `AiBackendId::MistralFim`, base `https://codestral.mistral.ai`, models `[("codestral-latest", "Codestral")]`

- [ ] **Step 1: Write the failing tests**

Append in `models/src/ai_catalog.rs` `#[cfg(test)]`:

```rust
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
    assert!(spec.builtin_models.iter().any(|(id, _)| *id == "codestral-latest"));
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
```

Keep existing `provider_group_classifies_subscription_and_agents` or replace it with the test above (do not leave two copies that disagree).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models backend_capabilities_match_protocol codestral_is_mistral_fim -- --nocapture`

Expected: FAIL compile (`AiBackendId` / `backend` field missing).

- [ ] **Step 3: Write minimal implementation**

In `models/src/ai_catalog.rs` add:

```rust
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
```

Replace `BuiltinProviderSpec` with:

```rust
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
```

Inside `builtin_providers()`, add constructors and rebuild `PROVIDERS` with them (keep every existing slug/URL/model list):

```rust
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
```

Assignment rules for existing rows:

- `backend`: `ollama` → `AiBackendId::Ollama`; `acp:opencode` / `acp:codex` → `acp(...)`; every other current NativeHttp row → `AiBackendId::OpenAiCompat`.
- `group`: `opencode-go`, `opencode-zen`, `nanogpt`, `zai-coding`, `xiaomi-plan` → `Subscription`; `ollama` → `Local`; ACP rows → `Agent`; all others → `Cloud`.
- `supports_thinking`: `true` only for `deepseek`.

Insert Codestral **before** the ACP rows:

```rust
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
```

`CODESTRAL_MODELS` is `&[("codestral-latest", "Codestral")]`.

`provider_kind`:

```rust
pub fn provider_kind(provider: &str) -> Option<AiProviderKind> {
    if let Some(spec) = builtin_providers().iter().find(|spec| spec.slug == provider) {
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
```

Replace `provider_group` body:

```rust
pub fn provider_group(provider: &str) -> AiProviderGroup {
    if let Some(spec) = builtin_providers().iter().find(|spec| spec.slug == provider) {
        return spec.group;
    }
    if provider.starts_with("acp:") {
        AiProviderGroup::Agent
    } else {
        AiProviderGroup::Cloud
    }
}
```

Call-site compile fixes (field → method):

- `services/src/app.rs`: `spec.kind == AiProviderKind::NativeHttp` → `spec.kind() == AiProviderKind::NativeHttp`
- `ui/.../catalog.rs`: `spec.kind` → `spec.kind()`, `spec.supports_model_refresh` → `spec.supports_model_refresh()`, `group: provider_group(spec.slug)` → `group: spec.group`
- `ui/.../settings_modal/sections.rs`: same `kind()` / `supports_model_refresh()`

- [ ] **Step 4: Run tests**

Run: `cargo test -p models backend_capabilities_match_protocol codestral_is_mistral_fim spec_fields_replace_slug_tables provider_group_reads_spec -- --nocapture`

Expected: PASS

Then: `cargo test -p models -- --nocapture`

Expected: PASS (existing catalog tests still pass; `builtin_catalog_has_current_us_and_cn_models` still finds previous slugs).

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs services/src/app.rs \
  ui/src/screens/workspace/components/agent_panel/catalog.rs \
  ui/src/layout/settings_modal/sections.rs
git commit -m "feat(models): add AI backend ids and catalog spec fields"
```

---

### Task 2: `active_completion`, custom `backend`, delete both slots

**Files:**
- Modify: `models/src/ai_catalog.rs`

**Interfaces:**
- Consumes: `AiBackendId`, `AiCatalogSettings`, `CustomNativeProvider`, `delete_custom_provider`.
- Produces:
  - `AiCatalogSettings.active_completion: Option<ActiveModel>`
  - `CustomNativeProvider.backend: AiBackendId` with `#[serde(default = "default_custom_backend")]`
  - `fn default_custom_backend() -> AiBackendId { AiBackendId::OpenAiCompat }`
  - `pub fn provider_backend(provider: &str, catalog: &AiCatalogSettings) -> Option<AiBackendId>`
  - `pub fn provider_offers_chat(provider: &str, catalog: &AiCatalogSettings) -> bool`
  - `pub fn provider_offers_complete(provider: &str, catalog: &AiCatalogSettings) -> bool`
  - `delete_custom_provider` clears `active` and `active_completion` when they point at the deleted id

- [ ] **Step 1: Write the failing tests**

```rust
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
    assert_eq!(
        provider_backend("ollama", &cat),
        Some(AiBackendId::Ollama)
    );
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
```

Update the existing `delete_custom_resets_active_when_it_was_selected` constructor to include `active_completion: None` and `backend: AiBackendId::OpenAiCompat` so it still compiles.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models default_custom_backend_is_openai_compat provider_backend_reads_spec delete_custom_clears_completion_slot -- --nocapture`

Expected: FAIL compile (`active_completion` / `backend` missing).

- [ ] **Step 3: Write minimal implementation**

```rust
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

pub fn provider_backend(provider: &str, catalog: &AiCatalogSettings) -> Option<AiBackendId> {
    if let Some(spec) = builtin_providers().iter().find(|spec| spec.slug == provider) {
        return spec.backend;
    }
    if let Some(custom) = catalog.custom_native.iter().find(|custom| custom.id == provider) {
        return Some(custom.backend);
    }
    if provider.starts_with("custom:") {
        return Some(AiBackendId::OpenAiCompat);
    }
    None
}

pub fn provider_offers_chat(provider: &str, catalog: &AiCatalogSettings) -> bool {
    provider_backend(provider, catalog)
        .is_some_and(|id| backend_capabilities(id).chat)
}

pub fn provider_offers_complete(provider: &str, catalog: &AiCatalogSettings) -> bool {
    provider_backend(provider, catalog)
        .is_some_and(|id| backend_capabilities(id).complete)
}

pub fn delete_custom_provider(cat: &mut AiCatalogSettings, id: &str) {
    cat.custom_native.retain(|provider| provider.id != id);
    if cat.active.as_ref().is_some_and(|active| active.provider == id) {
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
```

Fix every `CustomNativeProvider { ... }` literal in this file (and if compile fails, in `models/src/settings.rs` tests) to set `backend: AiBackendId::OpenAiCompat`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models default_custom_backend_is_openai_compat provider_backend_reads_spec delete_custom -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs models/src/settings.rs
git commit -m "feat(models): persist active_completion and custom backend"
```

---

### Task 3: Migrate legacy codestral/deepseek into `active_completion`

**Files:**
- Modify: `models/src/settings.rs` (`migrate_legacy_ai_fields`)

**Interfaces:**
- Consumes: `AiCatalogSettings.active_completion`, `CodeStralSettings`, `DeepSeekSettings`.
- Produces: `migrate_legacy_ai_fields` fills `active_completion` even when chat `active` is already set.

- [ ] **Step 1: Write the failing test**

In `models/src/settings.rs` tests:

```rust
#[test]
fn migrate_legacy_codestral_fills_active_completion_even_if_chat_active() {
    let json = serde_json::json!({
        "codestral": { "enabled": true, "model": "codestral-latest" },
        "deepseek": { "enabled": true, "model": "deepseek-chat" },
        "ai_catalog": {
            "active": { "provider": "openai", "model": "gpt-5.6-sol" }
        }
    });
    let mut settings: AppUiSettings = serde_json::from_value(json).unwrap();
    settings.migrate_legacy_ai_fields();
    let completion = settings
        .ai_catalog
        .active_completion
        .as_ref()
        .expect("active_completion");
    assert_eq!(completion.provider, "codestral");
    assert_eq!(completion.model, "codestral-latest");
    assert_eq!(settings.ai_catalog.active.as_ref().unwrap().provider, "openai");
    assert!(
        settings
            .ai_catalog
            .overrides
            .get("codestral")
            .is_some_and(|over| over.enabled)
    );
}

#[test]
fn migrate_legacy_deepseek_fills_completion_when_codestral_off() {
    let json = serde_json::json!({
        "deepseek": { "enabled": true, "model": "deepseek-chat" }
    });
    let mut settings: AppUiSettings = serde_json::from_value(json).unwrap();
    settings.migrate_legacy_ai_fields();
    let completion = settings
        .ai_catalog
        .active_completion
        .as_ref()
        .expect("active_completion");
    assert_eq!(completion.provider, "deepseek");
    assert_eq!(completion.model, "deepseek-chat");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models migrate_legacy_codestral_fills_active_completion migrate_legacy_deepseek_fills_completion -- --nocapture`

Expected: FAIL assert (`active_completion` is `None`).

- [ ] **Step 3: Write minimal implementation**

At the **end** of `migrate_legacy_ai_fields` (after the early-return chat block — so completion still runs when `active` is already set), call a private helper. Restructure as:

```rust
pub fn migrate_legacy_ai_fields(&mut self) {
    if self.ai_catalog.active.is_none() {
        self.migrate_legacy_chat_fields();
    }
    if self.ai_catalog.active_completion.is_none() {
        self.migrate_legacy_completion_fields();
    }
}
```

Rename the current body (the `legacy` array loop) to `migrate_legacy_chat_fields`. Add:

```rust
fn migrate_legacy_completion_fields(&mut self) {
    let codestral_model = self.codestral.model.trim();
    if self.codestral.enabled && !codestral_model.is_empty() {
        self.ai_catalog
            .overrides
            .entry("codestral".into())
            .or_default()
            .enabled = true;
        self.ai_catalog.active_completion = Some(crate::ActiveModel {
            provider: "codestral".into(),
            model: codestral_model.to_string(),
        });
        return;
    }
    let deepseek_model = self.deepseek.model.trim();
    if self.deepseek.enabled && !deepseek_model.is_empty() {
        self.ai_catalog.active_completion = Some(crate::ActiveModel {
            provider: "deepseek".into(),
            model: deepseek_model.to_string(),
        });
    }
}
```

Do not return early from the whole migrate fn just because `active` is `Some`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models migrate_legacy -- --nocapture`

Expected: PASS, including existing `migrate_legacy_deepseek_fills_catalog_active_and_override`.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs
git commit -m "feat(models): migrate codestral and deepseek into active_completion"
```

---

### Task 4: Copy legacy Codestral key into `shovel.lm.codestral`

**Files:**
- Modify: `services/src/app.rs` (`LEGACY_LM_KEYRING`)

**Interfaces:**
- Consumes: `hydrate_lm_keys` already copies `LEGACY_LM_KEYRING` when `shovel.lm.<id>` is empty.
- Produces: `("codestral", "shovel.codestral")` in that table so Codestral keys hydrate like DeepSeek.

- [ ] **Step 1: Write the failing test**

There is no unit test harness for keyring. Add a compile-visible constant test in `services/src/app.rs` under `#[cfg(test)]` (create a tests module if missing) **or** extend `storage/src/settings.rs` tests:

In `services/src/app.rs` make `LEGACY_LM_KEYRING` `pub(crate)` if needed and add at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::LEGACY_LM_KEYRING;

    #[test]
    fn legacy_keyring_includes_codestral() {
        assert!(
            LEGACY_LM_KEYRING
                .iter()
                .any(|(slug, service)| *slug == "codestral" && *service == "shovel.codestral")
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p services legacy_keyring_includes_codestral -- --nocapture`

Expected: FAIL assert.

- [ ] **Step 3: Write minimal implementation**

```rust
const LEGACY_LM_KEYRING: &[(&str, &str)] = &[
    ("deepseek", "shovel.deepseek"),
    ("openai", "shovel.openai"),
    ("groq", "shovel.groq"),
    ("openrouter", "shovel.openrouter"),
    ("xai", "shovel.xai"),
    ("mistral", "shovel.mistral"),
    ("ollama", "shovel.ollama"),
    ("codestral", "shovel.codestral"),
];
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p services legacy_keyring_includes_codestral -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add services/src/app.rs
git commit -m "feat(services): copy codestral key into shovel.lm.codestral"
```

---

### Task 5: Protocol backend trait and three impls (no HTTP send)

**Files:**
- Create: `acp-core/src/backends/mod.rs`
- Create: `acp-core/src/backends/openai.rs`
- Create: `acp-core/src/backends/ollama.rs`
- Create: `acp-core/src/backends/mistral_fim.rs`
- Modify: `acp-core/src/lib.rs` — `pub mod backends;` (keep `reorder_modules = false`: add next to `native_chat`)

**Interfaces:**
- Consumes: `models::{AiBackendId, AiCapabilities, backend_capabilities, normalize_native_chat_url}`.
- Produces:
  - `pub enum CompletionToken { Text(String), Done, Error(String) }` in `backends/mod.rs`
  - `pub struct CompleteRequest { pub backend: AiBackendId, pub base_url: String, pub api_key: String, pub model: String, pub prefix: String, pub suffix: Option<String>, pub schema_context: String }`
  - `pub trait AiBackend: Send + Sync` with the spec methods
  - `pub fn backend(id: AiBackendId) -> &'static dyn AiBackend`
  - `OpenAiCompatBackend`, `OllamaBackend`, `MistralFimBackend`
  - `NativeChatRequest` without `provider_slug`:

```rust
pub struct NativeChatRequest {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<NativeChatMessage>,
    pub backend: AiBackendId,
    pub supports_thinking: bool,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
}
```

`ui/.../requests.rs` `NativeChatParts` / `into_request` update in this task so the crate graph compiles.

- [ ] **Step 1: Write the failing tests**

Create `acp-core/src/backends/mod.rs` with tests only first (they will not compile):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use models::AiBackendId;
    use serde_json::json;

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
            b.chat_url("https://example.com/v1/chat/completions").unwrap(),
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
        assert!(msgs[0]["content"].as_str().unwrap().contains("SQL autocomplete"));
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
            vec![
                CompletionToken::Text("SEL".into()),
                CompletionToken::Done,
            ]
        );
    }

    #[test]
    fn mistral_fim_parse_one_shot() {
        let b = backend(AiBackendId::MistralFim);
        let json = r#"{"choices":[{"text":"id FROM t"}]}"#;
        assert_eq!(
            b.parse_complete(json),
            vec![CompletionToken::Text("id FROM t".into()), CompletionToken::Done]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p acp-core thinking_fields_follow_flag_not_slug -- --nocapture`

Expected: FAIL compile (`backends` module / `backend()` missing).

- [ ] **Step 3: Write minimal implementation**

`acp-core/src/lib.rs`: add `pub mod backends;` immediately after `pub mod native_chat;` (do not reorder other mods). Re-export:

```rust
pub use backends::{AiBackend, CompleteRequest, CompletionToken, backend};
```

`acp-core/src/backends/mod.rs`:

```rust
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
```

`openai.rs` (move URL/body/SSE logic out of `native_chat.rs` in this step, do not leave the slug copies behind):

- `chat_url`: current non-ollama branch of `chat_url` in `native_chat.rs`.
- `complete_url`: same as `chat_url`.
- `models_url`: current non-ollama branch of `models_url` in `native_runtime.rs` (`…/v1/models`, if base ends with `/v1` then `…/models`).
- `chat_body`: current `openai_request_body` but `thinking` / `reasoning_effort` only if `req.supports_thinking`. If `thinking_enabled`, insert `thinking: { "type": "enabled" }`. Always insert `reasoning_effort` normalized low/medium/high when `supports_thinking`.
- `complete_body`: `{ model, messages, max_tokens: 100, temperature: 0.1, stop: ["\n\n", ";", "```"], stream: true }` with messages copied from current `stream_deepseek` prompts in `ui/src/completion.rs` (system: SQL autocomplete engine rules + optional schema; user: `Complete after/between [CURSOR]`).
- `parse_chat`: current `parse_openai_sse`.
- `parse_complete`: same SSE walk, map `content` → `CompletionToken::Text`, ignore `reasoning_content`, `[DONE]` → `Done`, error → `Error`.
- `parse_models`: current `parse_openai_model_list` mapped to `AiModelEntry { id, label: String::new() }`.

`ollama.rs`:

- `chat_url` / `complete_url`: current ollama branch (`/api/chat`).
- `models_url`: current ollama `/api/tags`.
- `chat_body` / `complete_body`: `{ model, messages, stream: true }` with the same SQL messages as OpenAI for complete.
- `parse_chat`: current `parse_ollama_ndjson_line` (for a full payload, split lines).
- `parse_complete`: NDJSON content → `Text`, `done` → `Done`.
- `parse_models`: current tag list.

`mistral_fim.rs`:

- `chat_url` / `chat_body` / `parse_chat` / `models_url` / `parse_models` → `Err(unsupported(...))`.
- `complete_url`: `{normalized}/v1/fim/completions` unless base already contains `fim/completions`.
- `complete_body`: `{ model, prompt, suffix, max_tokens: 80, temperature: 0.2, top_p: 0.95, stop: ["\n\n", ";"] }` where prompt is prefix, or `-- Database schema:\n{schema}\n\n{prefix}` when schema non-empty.
- `parse_complete`: JSON `choices[0].text` or `choices[0].message.content`, trim CR/LF, then `[Text, Done]`. Empty text → `[Done]`.

Change `NativeChatRequest` in `native_chat.rs` as specified. Replace every struct literal in `native_chat.rs` tests: `provider_slug: "openai"` → `backend: AiBackendId::OpenAiCompat, supports_thinking: false`; deepseek thinking test uses `supports_thinking: true` and **must not** mention slug.

`stream_native_chat` calls `backend(req.backend)`. Replace `is_ollama = req.provider_slug == "ollama"` with `req.backend == AiBackendId::Ollama`. Body is `backend(req.backend).chat_body(&req)?`. URL is `backend(req.backend).chat_url(&req.base_url)?`. Delete `fn chat_url`, `fn openai_request_body`, and `fn ollama_request_body` from `native_chat.rs`. Tests that called `chat_url(&req)` call `backend(req.backend).chat_url(&req.base_url)` instead. `take_complete_line_events` still takes an `is_ollama: bool` sourced from `req.backend == AiBackendId::Ollama`.

`ui/.../requests.rs`:

```rust
struct NativeChatParts {
    base_url: String,
    api_key: String,
    model: String,
    backend: models::AiBackendId,
    supports_thinking: bool,
    thinking_enabled: bool,
    reasoning_effort: String,
}
```

`native_chat_parts`:

```rust
let backend = provider_backend(provider, &settings.ai_catalog)
    .expect("native http has backend");
let supports_thinking = builtin_providers()
    .iter()
    .find(|spec| spec.slug == provider)
    .is_some_and(|spec| spec.supports_thinking);
```

`into_request` copies `backend` and `supports_thinking`. Drop `provider_slug`.

`acp-core` tests that construct `NativeChatRequest` with `..openai.clone()` still work if `backend` is on the struct.

- [ ] **Step 4: Run tests**

Run: `cargo test -p acp-core openai_chat_url_does_not_double_v1 thinking_fields_follow_flag_not_slug ollama_urls mistral_fim_complete_url openai_complete_body openai_complete_parse mistral_fim_parse -- --nocapture`

Expected: PASS

Run: `cargo test -p acp-core -- --nocapture`

Expected: PASS (old `deepseek_request_includes_thinking_fields` updated to flag, `chat_url_openai_and_ollama` uses `backend`).

Run: `cargo test -p ui --lib -- --nocapture` is heavy; at least `cargo check -p ui`.

Expected: `ui` compiles with the new `NativeChatRequest`.

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/backends acp-core/src/lib.rs acp-core/src/native_chat.rs \
  ui/src/screens/workspace/components/agent_panel/requests.rs
git commit -m "feat(acp-core): add protocol backends and drop slug chat dispatch"
```

Do not change `refresh_provider_models` in this task. Leave `native_runtime.rs` slug URL helper for Task 6.

---

### Task 6: Refresh models by `AiBackendId`

**Files:**
- Modify: `acp-core/src/native_runtime.rs`
- Modify: `ui/src/screens/workspace/components/agent_panel/catalog.rs` (`refresh_picker_models`)
- Modify: `ui/src/layout/settings_modal/sections.rs` (`refresh_catalog_models`)

**Interfaces:**
- Consumes: `backend(id).models_url` / `parse_models`.
- Produces: `pub async fn refresh_provider_models(backend: AiBackendId, base_url: &str, api_key: &str) -> Result<Vec<AiModelEntry>, String>`

- [ ] **Step 1: Write the failing test**

In `native_runtime.rs` tests (add `#[cfg(test)]` module if missing):

```rust
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
    assert!(backend(AiBackendId::MistralFim)
        .models_url("https://codestral.mistral.ai")
        .is_err());
}
```

Move `parse_openai_model_list` tests if they exist onto `backend().parse_models`. Add:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p acp-core parse_models_openai_and_ollama -- --nocapture`

Expected: PASS if Task 5 already implemented `parse_models` (then this test is a regression lock). The signature change in Step 3 is the real break: `cargo check -p ui` fails until call sites take `AiBackendId`.

- [ ] **Step 3: Write minimal implementation**

Replace `refresh_provider_models` and delete `models_url(slug, …)` / slug `if`:

```rust
pub async fn refresh_provider_models(
    backend_id: models::AiBackendId,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<AiModelEntry>, String> {
    let backend = crate::backends::backend(backend_id);
    let url = backend.models_url(base_url)?;
    // existing reqwest GET + auth_headers + status handling
    let body = /* text */;
    backend.parse_models(&body)
}
```

Call sites resolve backend **before** await (drop settings borrow):

`catalog.rs` `refresh_picker_models`:

```rust
fn refresh_picker_models(slug: String) {
    let settings = crate::app_state::APP_UI_SETTINGS();
    let base_url = native_base_url(&settings, &slug);
    let api_key = settings.lm_api_key(&slug);
    let Some(backend) = provider_backend(&slug, &settings.ai_catalog) else {
        crate::app_state::toast_error("This provider cannot refresh models.".into());
        return;
    };
    spawn(async move {
        match services::refresh_provider_models(backend, &base_url, &api_key).await {
            Ok(models) => merge_refreshed_models(&slug, models),
            Err(err) => crate::app_state::toast_error(err),
        }
    });
}
```

Same pattern in `refresh_catalog_models` in `sections.rs`. Hide/disable Refresh when `!spec.supports_model_refresh()` (already true for Codestral).

- [ ] **Step 4: Run tests**

Run: `cargo test -p acp-core parse_models_openai_and_ollama models_url_comes_from_backend -- --nocapture`

Expected: PASS

Run: `cargo check -p ui -p services`

Expected: success.

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/native_runtime.rs \
  ui/src/screens/workspace/components/agent_panel/catalog.rs \
  ui/src/layout/settings_modal/sections.rs
git commit -m "feat(acp-core): refresh provider models by backend id"
```

---

### Task 7: `complete_sql` runner

**Files:**
- Create: `acp-core/src/native_complete.rs`
- Modify: `acp-core/src/lib.rs` — `pub mod native_complete; pub use native_complete::complete_sql;`
- Modify: `acp/src/lib.rs` — `pub use acp_core::complete_sql;` and `CompletionToken`
- Modify: `services/src/lib.rs` — re-export `complete_sql`, `CompletionToken`, `CompleteRequest`
- Modify: `services/tests/facade_smoke.rs`

**Interfaces:**
- Consumes: `CompleteRequest`, `backend().complete_url/complete_body/parse_complete`, existing `auth_headers` (make `pub(crate)` from `native_chat` or `native_runtime`).
- Produces: `pub fn complete_sql(req: CompleteRequest) -> tokio::sync::mpsc::UnboundedReceiver<CompletionToken>`
  - 15s request timeout
  - does **not** read or write `NATIVE_CHAT_CANCEL`
  - OpenAI/Ollama: stream bytes, parse with `parse_complete` per line (SSE vs NDJSON from `req.backend == Ollama`)
  - MistralFim: one-shot POST, `parse_complete` on full body
  - 401/403 → `Error("Auth failed")` then `Done`
  - dropping the receiver aborts the spawned task

- [ ] **Step 1: Write the failing tests**

In `native_complete.rs`:

```rust
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
        let _rx = complete_sql(req);
    }
}
```

Do not hit the network. Parse mapping is already tested in Task 5.

```rust
#[test]
fn complete_does_not_set_native_chat_cancel() {
    crate::native_chat::clear_native_chat_cancel();
    assert!(!crate::native_chat::native_chat_cancel_requested());
}
```

Export `clear_native_chat_cancel` / `native_chat_cancel_requested` from `acp-core` if they are not already `pub`. Make `auth_headers` in `native_chat.rs` `pub(crate)` so `native_complete.rs` can reuse it.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p acp-core complete_sql_is_exported_shape -- --nocapture`

Expected: FAIL compile (`complete_sql` missing).

- [ ] **Step 3: Write minimal implementation**

`native_complete.rs`:

```rust
use std::time::Duration;
use tokio::sync::mpsc;

use crate::backends::{backend, CompleteRequest, CompletionToken};

const COMPLETE_TIMEOUT: Duration = Duration::from_secs(15);

pub fn complete_sql(req: CompleteRequest) -> mpsc::UnboundedReceiver<CompletionToken> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        if let Err(err) = run_complete(req, &tx).await {
            let _ = tx.send(CompletionToken::Error(err));
        }
        let _ = tx.send(CompletionToken::Done);
    });
    rx
}
```

`run_complete`:

1. `let b = backend(req.backend);`
2. `let url = b.complete_url(&req.base_url)?;`
3. `let body = b.complete_body(&req)?;`
4. Build reqwest client with `connect_timeout` 15s and `timeout` 15s.
5. POST with the same Bearer headers as chat (`pub(crate) auth_headers` — move `auth_headers` in `native_chat.rs` to `pub(crate)`).
6. Status 401/403 → `Err("Auth failed".into())`.
7. If `req.backend == AiBackendId::MistralFim`: `let text = response.text().await?;` then send each `b.parse_complete(&text)` token except a trailing `Done` (the wrapper sends `Done`).
8. Else: byte stream, line buffer. For Ollama split on `\n` and `parse_complete` each line; for OpenAI reuse SSE `data:` lines. Forward `Text`/`Error`; stop on `Done` without double-sending if the wrapper always sends `Done`.

Do not call `clear_native_chat_cancel` or `request_native_chat_cancel`.

`acp/src/lib.rs` add to the `acp_core` re-export list: `complete_sql`, `CompleteRequest`, `CompletionToken`.

`services/src/lib.rs` add the same three names to the `pub use acp::{...}` block.

`facade_smoke.rs`:

```rust
let _ = &services::complete_sql;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p acp-core complete_does_not_set_native_chat_cancel -- --nocapture`

Expected: PASS

Run: `cargo test -p services facade_lists -- --nocapture`

Expected: PASS (`complete_sql` referenced).

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/native_complete.rs acp-core/src/lib.rs acp-core/src/native_chat.rs \
  acp/src/lib.rs services/src/lib.rs services/tests/facade_smoke.rs
git commit -m "feat(acp-core): add native SQL completion runner"
```

---

### Task 8: UI completion wrapper without `reqwest`

**Files:**
- Modify: `ui/src/completion.rs` (replace HTTP providers)
- Modify: `ui/Cargo.toml` (remove `reqwest` if unused)
- Modify: `ui/src/screens/workspace/components/sql_editor.rs` only if `CompletionToken` import path changes

**Interfaces:**
- Consumes: `services::{complete_sql, CompleteRequest, CompletionToken}`, `provider_backend`, `provider_offers_complete`, `is_native_http_ready`, `native_http_provider_enabled`, `active_completion`.
- Produces: `CompletionService::new` / `is_empty` / `stream_completion(prefix, suffix, schema)` with the same signatures `sql_editor.rs` already calls. No DeepSeek→CodeStral fallback. `is_empty` is true when `active_completion` is missing, backend lacks `complete`, provider disabled, or credentials missing.

- [ ] **Step 1: Write the failing test**

`ui` has no completion unit tests today. Add a small pure helper in `completion.rs` and test it:

```rust
pub(crate) fn complete_request_from_settings(
    settings: &AppUiSettings,
) -> Option<services::CompleteRequest> { ... }

#[cfg(test)]
mod tests {
    use super::*;
    use models::{ActiveModel, AiCatalogSettings, AiProviderOverride, AppUiSettings};

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
```

This test fails until the helper exists. `set_lm_api_key` already exists on `AppUiSettings`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ui complete_request_uses_active_completion -- --nocapture`

Expected: FAIL compile (`complete_request_from_settings` missing).

- [ ] **Step 3: Write minimal implementation**

Replace `ui/src/completion.rs` with:

- Re-export `pub use services::CompletionToken;`
- `complete_request_from_settings` as above:
  - read `settings.ai_catalog.active_completion`
  - `provider_offers_complete`
  - `native_http_provider_enabled` (custom is enabled)
  - `native_http_has_credentials` / `is_native_http_ready`
  - `provider_backend`
  - base URL: duplicate the 15-line resolve from `native_chat_parts`'s `native_base_url` into `completion.rs` (custom row, then override, then builtin default, through `normalize_native_chat_url`). Do not import `pub(super) native_base_url` from the agent panel.
- `CompletionService { request: Option<CompleteRequest> }`
- `new`: `Self { request: complete_request_from_settings(settings) }`
- `is_empty`: `self.request.is_none()`
- `stream_completion`: if `None`, send `Done` on a new channel; else clone request, set `prefix` / `suffix` / `schema_context`, return `services::complete_sql(req)`

Delete `reqwest`, `CodeStralProvider`, `DeepSeekProvider`, `CODESTRAL_API_URL`, fallback loop.

`sql_editor.rs` keeps `use crate::completion::{CompletionService, CompletionToken};` if `completion.rs` re-exports `CompletionToken`.

Remove `reqwest.workspace = true` from `ui/Cargo.toml`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p ui complete_request_uses_active_completion complete_request_none_without_slot -- --nocapture`

Expected: PASS

Run: `cargo check -p ui`

Expected: success, no `reqwest` in `ui`.

- [ ] **Step 5: Commit**

```bash
git add ui/src/completion.rs ui/Cargo.toml
git commit -m "feat(ui): route SQL completion through services backends"
```

---

### Task 9: Settings + chat picker use catalog capabilities

**Files:**
- Modify: `ui/src/app_state/mod.rs` — `set_active_completion`
- Modify: `ui/src/layout/settings_modal/sections.rs` — SQL autocomplete provider/model; Codestral is a normal catalog card (already listed if NativeHttp cards iterate builtins)
- Modify: `ui/src/screens/workspace/components/agent_panel/catalog.rs` — chat rows only when `provider_offers_chat` (or spec backend has `chat`); do not list Codestral as a chat target

**Interfaces:**
- Consumes: `active_completion`, `provider_offers_chat`, `provider_offers_complete`, `resolve_picker_models`.
- Produces:
  - `pub fn set_active_completion(provider: String, model: String)` writing `ai_catalog.active_completion`
  - Settings: two `<select>`s (provider, model) bound with `onchange`, writing via `set_active_completion`
  - Chat picker omits complete-only backends

- [ ] **Step 1: Write the failing test**

Add a pure helper in `models` (preferred) so UI stays thin — if not already from Task 2, test picker lists in `models/src/ai_catalog.rs`:

```rust
pub fn completion_picker_ids(catalog: &AiCatalogSettings) -> Vec<String> {
    let mut ids = Vec::new();
    for spec in builtin_providers() {
        if provider_offers_complete(spec.slug, catalog)
            && native_http_provider_enabled(catalog, spec.slug)
        {
            ids.push(spec.slug.to_string());
        }
    }
    for custom in &catalog.custom_native {
        if provider_offers_complete(&custom.id, catalog) {
            ids.push(custom.id.clone());
        }
    }
    ids
}

pub fn chat_picker_native_ids(catalog: &AiCatalogSettings) -> Vec<String> {
    builtin_providers()
        .iter()
        .filter(|spec| {
            provider_offers_chat(spec.slug, catalog)
                && native_http_provider_enabled(catalog, spec.slug)
        })
        .map(|spec| spec.slug.to_string())
        .collect()
}
```

Tests:

```rust
#[test]
fn pickers_split_chat_and_complete() {
    let mut cat = AiCatalogSettings::default();
    for slug in ["openai", "codestral", "ollama"] {
        cat.overrides.insert(
            slug.into(),
            AiProviderOverride {
                enabled: true,
                ..Default::default()
            },
        );
    }
    let complete = completion_picker_ids(&cat);
    assert!(complete.contains(&"codestral".into()));
    assert!(complete.contains(&"openai".into()));
    let chat = chat_picker_native_ids(&cat);
    assert!(chat.contains(&"openai".into()));
    assert!(!chat.contains(&"codestral".into()));
    assert!(!chat.iter().any(|id| id.starts_with("acp:")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models pickers_split_chat_and_complete -- --nocapture`

Expected: FAIL compile if helpers missing.

- [ ] **Step 3: Write minimal implementation**

Add the two helpers in `ai_catalog.rs`.

`set_active_completion` next to `set_active_model`:

```rust
pub fn set_active_completion(provider: String, model: String) {
    update_ui_settings(|current| {
        current.ai_catalog.active_completion = Some(ActiveModel { provider, model });
    });
}
```

Chat picker row builder in `catalog.rs`: skip spec when `!provider_offers_chat(spec.slug, &settings.ai_catalog)` (ACP section unchanged). Custom rows: skip when `!provider_offers_chat`.

Settings Language models section: after builtin cards, add “SQL autocomplete”:

- Provider `<select>` options = `completion_picker_ids` plus a disabled empty option “Off” with value `""`.
- On `onchange`: if empty, `update_ui_settings(|s| s.ai_catalog.active_completion = None)`; else pick first model from `resolve_picker_models` for that provider and `set_active_completion`.
- Model `<select>` from that provider’s resolved models; `onchange` writes `set_active_completion(provider, model)`.
- Use `onchange`, not `oninput`.

If Codestral is not already on the builtin NativeHttp settings cards (`filter kind NativeHttp`), it will appear automatically after Task 1. Confirm the card shows API key + enable + models and **no** Refresh (`!supports_model_refresh()`).

Remove the standalone CodeStral settings block (`settings.codestral.api_key` inputs) if it still exists, without deleting the serde struct. Point the user at the catalog card.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models pickers_split_chat_and_complete -- --nocapture`

Expected: PASS

Run: `cargo check -p ui`

Expected: success.

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs ui/src/app_state/mod.rs \
  ui/src/layout/settings_modal/sections.rs \
  ui/src/screens/workspace/components/agent_panel/catalog.rs
git commit -m "feat(ui): split chat and SQL completion pickers"
```

---

### Task 10: Workspace fmt, clippy, tests

**Files:** none new; fix whatever Task 1–9 left behind.

**Interfaces:** none.

- [ ] **Step 1: Run format check**

Run: `cargo fmt --all -- --check`

Expected: PASS. If it fails, run `cargo fmt --all` and include the diff in the commit.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS. Fix any new warnings in the files this plan touched (no drive-by).

- [ ] **Step 3: Tests**

Run: `cargo test --workspace`

Expected: PASS. Confirm in the log:

- `codestral_is_mistral_fim_complete_only`
- `thinking_fields_follow_flag_not_slug`
- `migrate_legacy_codestral_fills_active_completion_even_if_chat_active`
- `complete_request_uses_active_completion_not_chat`
- `pickers_split_chat_and_complete`
- `facade` smoke still sees `complete_sql`

Grep the tree (exclude docs) for `provider_slug ==` and `slug == "ollama"` in `acp-core` and `ui/src/completion.rs`. Both must be gone.

- [ ] **Step 4: Commit only if fmt/clippy produced diffs**

```bash
git add -u
git commit -m "chore: fmt and clippy after modular AI backends"
```

Skip the commit if the working tree is clean.

---

## Self-review (spec coverage)

| Spec requirement | Task |
| --- | --- |
| `AiBackendId` + capabilities from backend | 1 |
| Spec `backend` / `group` / `supports_thinking`; Codestral row | 1 |
| `kind()` / refresh derived; `provider_group` from spec | 1 |
| `active_completion`; custom `backend`; delete both slots | 2 |
| `provider_backend` / offers_chat / offers_complete | 2 |
| Legacy codestral/deepseek → `active_completion` | 3 |
| `shovel.codestral` → `shovel.lm.codestral` | 4 |
| Trait + three impls; thinking flag not slug | 5 |
| `NativeChatRequest` without slug dispatch | 5 |
| Refresh by backend id | 6 |
| `complete_sql`, 15s, no chat cancel | 7 |
| Facade export | 7 |
| UI completion without reqwest; uses `active_completion` | 8 |
| Chat picker excludes Codestral; settings completion selects | 9 |
| fmt/clippy/workspace tests | 10 |
| No per-vendor modules, no inventory, no new crate, no ACP-as-backend | all |
| Editor menu/ghost/variants not implemented | all (out of scope) |
)
