# Native AI Provider Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace per-vendor settings structs and ACP-child DeepSeek/Ollama chat with a Zed-style catalog, in-process HTTP for URL providers, an in-chat model picker, and ACP only for OpenCode/Codex/custom stdio.

**Architecture:** Types and migration live in `models`. Keys stay in `storage` under `shovel.lm.<provider_id>`. `acp-core` gains a native chat backend that emits existing `AcpEvent`s. `services` re-exports `native_chat_prompt` / `refresh_provider_models` / `cancel_acp_prompt`. `ui` only talks to `models` and `services`. Same-provider model change writes `ActiveModel` and the next HTTP body uses that id. Crossing onto ACP disconnects/reconnects the child; the chat thread is unchanged.

**Tech Stack:** Rust nightly (workspace pin), Dioxus 0.7, serde, reqwest, tokio, keyring, grass SCSS.

**Spec:** `docs/superpowers/specs/2026-08-27-ai-provider-catalog-design.md`

## Global Constraints

- Dioxus 0.7 only (`use_signal`, `use_effect`, `#[component]`). No `cx` / `Scope` / `use_state`.
- Never hold a signal read/write across `.await`.
- `ui` may import `models` and `services` only. No `reqwest` from `ui`.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `rustfmt.toml`: `max_width = 100`, `imports_granularity = "Crate"`, `reorder_modules = false`.
- API keys: `#[serde(skip_serializing)]`, keyring `shovel.lm.<provider_id>`, existing fallback file if keyring is dead.
- `<select>` uses `onchange`, never `oninput` (WebKitGTK).
- Do not implement spec B (agent-run SQL, schema index, DB Q&A).
- Do not change the CodeStral/DeepSeek inline completion path.
- Desktop chat must not spawn `shovel acp-agent deepseek|ollama`. Those CLI entrypoints may remain for headless use.
- Do not add Anthropic/Google/Bedrock HTTP APIs.

## File structure

- Create: `models/src/ai_catalog.rs` — catalog types, builtin specs, `resolve_picker_models`, `normalize_native_chat_url`, `migrate_legacy_ai_fields`
- Modify: `models/src/lib.rs` — `mod ai_catalog; pub use ai_catalog::*;`
- Modify: `models/src/settings.rs` — `AppUiSettings.ai_catalog`, keep legacy fields for deserialize only (`skip_serializing`)
- Modify: `models/src/settings_roundtrip.rs` — catalog present after empty/legacy JSON
- Modify: `storage/src/settings.rs` — call migrate on load; copy old keyring names to `shovel.lm.<slug>`
- Modify: `services/src/app.rs` — hydrate/save keys by catalog provider id
- Create: `acp-core/src/native_chat.rs` — `NativeChatBackend`, OpenAI-compat + Ollama, stream → events
- Modify: `acp-core/src/lib.rs` — `pub mod native_chat;`
- Modify: `acp-core/src/runtime.rs` (or a thin sibling) — `native_chat_prompt` / cancel shares `cancel_acp_prompt` facade
- Modify: `services/src/lib.rs` — re-export native chat + refresh
- Modify: `ui/src/screens/workspace/components/agent_panel/composer.rs` — picker control
- Modify: `ui/src/screens/workspace/components/agent_panel/mod.rs` — native send path; Providers vs ACP
- Modify: `ui/src/layout/settings_modal.rs` — Language models cards + custom CRUD
- Modify: `ui/src/app_state/mod.rs` — `set_active_model`, catalog mutators
- Modify: `styles/components/_agent-panel.scss` — picker menu
- Do not delete `acp-core/src/deepseek.rs` / `ollama.rs` CLI agents in this plan

---

### Task 1: Catalog types, picker merge, URL normalize

**Files:**
- Create: `models/src/ai_catalog.rs`
- Modify: `models/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct ActiveModel { pub provider: String, pub model: String }`
  - `pub struct AiModelEntry { pub id: String, pub label: String }`
  - `pub struct AiProviderOverride { pub enabled: bool, pub base_url: String, pub extra_models: Vec<AiModelEntry>, pub hidden_builtin_ids: Vec<String>, pub thinking_enabled: bool, pub reasoning_effort: String }`
  - `pub struct CustomNativeProvider { pub id: String, pub name: String, pub base_url: String, pub models: Vec<AiModelEntry> }`
  - `pub struct AiCatalogSettings { pub active: Option<ActiveModel>, pub overrides: BTreeMap<String, AiProviderOverride>, pub custom_native: Vec<CustomNativeProvider> }`
  - `pub struct BuiltinProviderSpec { pub slug: &'static str, pub label: &'static str, pub kind: AiProviderKind, pub default_base_url: &'static str, pub builtin_models: &'static [(&'static str, &'static str)], pub supports_model_refresh: bool }`
  - `pub enum AiProviderKind { NativeHttp, Acp }`
  - `pub fn builtin_providers() -> &'static [BuiltinProviderSpec]`
  - `pub fn resolve_picker_models(builtin: &[AiModelEntry], extra: &[AiModelEntry], hidden: &[String]) -> Vec<AiModelEntry>`
  - `pub fn normalize_native_chat_url(base: &str, default_base: &str) -> String`
  - `impl Default` for override (enabled false, empty urls, thinking false, reasoning `"medium"`) and catalog (active None, empty maps)

- [ ] **Step 1: Write the failing test**

In `models/src/ai_catalog.rs` under `#[cfg(test)]`:

```rust
#[test]
fn resolve_picker_models_hides_builtins_and_appends_extra_without_dupes() {
    let builtin = vec![
        AiModelEntry { id: "gpt-4o".into(), label: String::new() },
        AiModelEntry { id: "gpt-4o-mini".into(), label: String::new() },
    ];
    let extra = vec![
        AiModelEntry { id: "gpt-4o".into(), label: "dup".into() },
        AiModelEntry { id: "my-ft".into(), label: "Fine-tune".into() },
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models resolve_picker_models_hides -- --nocapture`

Expected: FAIL compiling (`ai_catalog` module missing) or unresolved `resolve_picker_models`.

- [ ] **Step 3: Write minimal implementation**

`models/src/ai_catalog.rs`: define the structs with `#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]` and `#[serde(default)]` on override/catalog. `resolve_picker_models`: iterate builtin skipping `hidden`, then extra skipping ids already present. `normalize_native_chat_url`: trim, if empty use `default_base`, `trim_end_matches('/')`. `builtin_providers()` returns DeepSeek, OpenAI, Groq, OpenRouter, xAI, Mistral, Ollama (NativeHttp) plus `acp:opencode` and `acp:codex` (Acp, empty builtin models, `supports_model_refresh: false`). Ollama and all NativeHttp builtins except none: `supports_model_refresh: true` for HTTP slugs including `ollama`.

Builtin model ids (label empty unless noted):

- deepseek: `deepseek-chat`, `deepseek-v4-pro`, `deepseek-v4-flash`
- openai: `gpt-4.1`, `gpt-4.1-mini`, `gpt-4o`, `gpt-4o-mini`, `o4-mini`
- groq: `llama-3.3-70b-versatile`, `openai/gpt-oss-120b`, `qwen/qwen3-32b`
- openrouter: `openai/gpt-4o`, `anthropic/claude-sonnet-4`, `google/gemini-2.5-pro`
- xai: `grok-4`, `grok-3`, `grok-3-mini`
- mistral: `mistral-large-latest`, `codestral-latest`, `mistral-small-latest`
- ollama: empty slice (filled by Refresh / extra_models)

Default base URLs: DeepSeek `https://api.deepseek.com`, OpenAI `https://api.openai.com`, Groq `https://api.groq.com/openai`, OpenRouter `https://openrouter.ai/api`, xAI `https://api.x.ai`, Mistral `https://api.mistral.ai`, Ollama `http://localhost:11434`.

`models/src/lib.rs`: `mod ai_catalog; pub use ai_catalog::*;` after `mod settings` without reordering other modules (`reorder_modules = false`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p models resolve_picker_models_hides normalize_native_chat_url -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs models/src/lib.rs
git commit -m "feat(models): add AI provider catalog types and picker merge"
```

---

### Task 2: Persist catalog on `AppUiSettings` and migrate legacy fields

**Files:**
- Modify: `models/src/settings.rs`
- Modify: `models/src/settings_roundtrip.rs`
- Test: existing `#[cfg(test)]` in `models/src/settings.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `AppUiSettings.ai_catalog: AiCatalogSettings`. `pub fn migrate_legacy_ai_fields(&mut self)` on `AppUiSettings`. Legacy `deepseek`/`openai`/`groq`/`openrouter`/`xai`/`mistral`/`ollama` still deserialize (`#[serde(default)]`) and use `#[serde(skip_serializing)]`.

- [ ] **Step 1: Write the failing test**

Add to `models/src/settings.rs` tests:

```rust
#[test]
fn migrate_legacy_deepseek_fills_catalog_active_and_override() {
    let json = r#"{
        "theme":"Dark",
        "deepseek":{
            "enabled":true,
            "base_url":"https://api.deepseek.com",
            "model":"deepseek-chat",
            "thinking_enabled":true,
            "reasoning_effort":"high"
        }
    }"#;
    let mut settings: AppUiSettings = serde_json::from_str(json).unwrap();
    settings.migrate_legacy_ai_fields();
    let active = settings.ai_catalog.active.expect("active");
    assert_eq!(active.provider, "deepseek");
    assert_eq!(active.model, "deepseek-chat");
    let over = settings.ai_catalog.overrides.get("deepseek").expect("override");
    assert!(over.enabled);
    assert_eq!(over.reasoning_effort, "high");
    assert!(over.thinking_enabled);
    let dumped = serde_json::to_value(&settings).unwrap();
    assert!(dumped.get("deepseek").is_none());
    assert!(dumped.get("ai_catalog").is_some());
}

#[test]
fn catalog_secrets_are_not_in_json() {
    let mut settings = AppUiSettings::default();
    settings.deepseek.api_key = "sk-legacy".into();
    let dumped = serde_json::to_string(&settings).unwrap();
    assert!(!dumped.contains("sk-legacy"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models migrate_legacy_deepseek -- --nocapture`

Expected: FAIL (`ai_catalog` / `migrate_legacy_ai_fields` missing).

- [ ] **Step 3: Write minimal implementation**

Add `pub ai_catalog: AiCatalogSettings` to `AppUiSettings` with `#[serde(default)]`. Default: `AiCatalogSettings::default()`.

Keep existing vendor structs. Mark them `#[serde(default, skip_serializing)]`.

```rust
impl AppUiSettings {
    pub fn migrate_legacy_ai_fields(&mut self) {
        if self.ai_catalog.active.is_some() {
            return;
        }
        // For each known slug, if the legacy struct has a non-empty model
        // or enabled/key, write AiProviderOverride { enabled, base_url,
        // extra_models if model not in builtin list, thinking_*, reasoning_* }.
        // Set active to the first enabled legacy with a model, in order:
        // deepseek, openai, groq, openrouter, xai, mistral, ollama.
    }
}
```

Call `migrate_legacy_ai_fields` from `settings_roundtrip` tests that load empty objects only if you also call it in `storage` load (Task 3). For this task, tests call it explicitly.

Fix `AppUiSettings { ... }` struct literals in `settings.rs` tests and `settings_roundtrip.rs` by using `..AppUiSettings::default()` already present, or add `ai_catalog: AiCatalogSettings::default()`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models settings -- --nocapture`

Expected: PASS (including roundtrip and the new migration tests).

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs models/src/settings_roundtrip.rs
git commit -m "feat(models): persist ai_catalog and migrate legacy provider fields"
```

---

### Task 3: Load/save keys as `shovel.lm.<id>` and migrate old keyring names

**Files:**
- Modify: `storage/src/settings.rs`
- Modify: `services/src/app.rs`

**Interfaces:**
- Consumes: `AppUiSettings::migrate_legacy_ai_fields`, existing `load_lm_api_key` / `save_lm_api_key`.
- Produces: `load_app_ui_settings` returns migrated catalog. Hydrate copies `shovel.deepseek` → `shovel.lm.deepseek` when the new service is empty. Save writes `shovel.lm.<slug>` for every builtin override and each `custom:*`. JSON save is not blocked if keyring fails: warn via returned `Result` only after JSON succeeded, same partial-success string shape as today.

- [ ] **Step 1: Write the failing test**

If `storage` has no unit hook for migrate-on-load, add a models-level test already done; for storage add:

```rust
#[test]
fn lm_keyring_service_name_is_stable() {
    assert_eq!(
        super::lm_service_name("deepseek"),
        "shovel.lm.deepseek"
    );
    assert_eq!(
        super::lm_service_name("custom:abc"),
        "shovel.lm.custom:abc"
    );
}
```

Put `pub(crate) fn lm_service_name(provider_id: &str) -> String` in `storage/src/settings.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p storage lm_keyring_service_name -- --nocapture`

Expected: FAIL (function missing).

- [ ] **Step 3: Write minimal implementation**

```rust
pub(crate) fn lm_service_name(provider_id: &str) -> String {
    format!("shovel.lm.{provider_id}")
}
```

In `load_app_ui_settings`, after JSON parse, `settings.migrate_legacy_ai_fields()`.

In `services/src/app.rs` `load_app_startup_settings`: after load, for slugs `deepseek`, `openai`, `groq`, `openrouter`, `xai`, `mistral`, `ollama` (and codestral unchanged): if `load_lm_api_key(&lm_service_name(slug))` is empty, try the old service (`shovel.deepseek` etc. via existing helpers), and if found `save_lm_api_key` to the new name.

Hydrate in-memory keys into a side map is **not** stored on `AiCatalogSettings` (no api_key field). Keep hydrating `deepseek.api_key` / `openai.api_key` for one release **or** introduce `APP_LM_KEYS: GlobalSignal<BTreeMap<String,String>>` in Task 6. For this task, hydrate into a new field on a runtime-only struct is too much; instead add `#[serde(skip)] pub lm_keys: BTreeMap<String, String>` on `AppUiSettings` **only if** that does not serialize (skip) and Default empty. Spec said keys in memory not JSON: `#[serde(skip)] pub lm_keys: BTreeMap<String, String>` on `AppUiSettings`.

Add `lm_keys` in this task with a models test that serialize omits it. Update `migrate` tests if struct literals break.

`save_app_ui_settings_with_secrets`: save JSON first (catalog without keys), then for each key in `lm_keys` call `save_lm_api_key`. On keyring error use existing fallback inside `save_lm_api_key`; collect warnings.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models -p storage -p services -- --nocapture`

Expected: PASS for the touched tests. `services` facade_smoke still compiles.

- [ ] **Step 5: Commit**

```bash
git add models/src/settings.rs storage/src/settings.rs services/src/app.rs
git commit -m "feat(storage): migrate AI keys to shovel.lm.<provider> and catalog load"
```

---

### Task 4: Native HTTP chat backend in `acp-core`

**Files:**
- Create: `acp-core/src/native_chat.rs`
- Modify: `acp-core/src/lib.rs`

**Interfaces:**
- Consumes: `normalize_native_chat_url`, `ActiveModel`.
- Produces:
  - `pub struct NativeChatRequest { pub base_url: String, pub api_key: String, pub model: String, pub messages: Vec<NativeChatMessage>, pub provider_slug: String, pub thinking_enabled: bool, pub reasoning_effort: String }`
  - `pub struct NativeChatMessage { pub role: String, pub content: String }`
  - `pub enum NativeChatEvent { Delta(String), Thought(String), Finished, Error(String) }`
  - `pub async fn stream_native_chat(req: NativeChatRequest) -> Result<impl Stream<Item = NativeChatEvent>, String>`
  - Chat URL: OpenAI-compat `format!("{}/v1/chat/completions", normalize_native_chat_url(base, base))` unless `base` contains `chat/completions`. Ollama: if slug `ollama`, POST `{normalize}/api/chat` (if base already ends with `/api`, do not add another `/api`).

- [ ] **Step 1: Write the failing test**

In `acp-core/src/native_chat.rs` tests, parse a fixture SSE without the network:

```rust
#[test]
fn parse_openai_sse_emits_deltas_and_finished() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    let events = parse_openai_sse(body);
    assert_eq!(
        events,
        vec![
            NativeChatEvent::Delta("Hel".into()),
            NativeChatEvent::Delta("lo".into()),
            NativeChatEvent::Finished,
        ]
    );
}

#[test]
fn openai_request_json_uses_active_model() {
    let req = NativeChatRequest {
        base_url: "https://api.openai.com".into(),
        api_key: "sk".into(),
        model: "gpt-4o-mini".into(),
        messages: vec![NativeChatMessage { role: "user".into(), content: "hi".into() }],
        provider_slug: "openai".into(),
        thinking_enabled: false,
        reasoning_effort: "medium".into(),
    };
    let v = openai_request_body(&req);
    assert_eq!(v["model"], "gpt-4o-mini");
    assert_eq!(v["stream"], true);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p acp-core parse_openai_sse -- --nocapture`

Expected: FAIL (module missing).

- [ ] **Step 3: Write minimal implementation**

Implement `parse_openai_sse` over lines starting with `data:`. Skip empty. `[DONE]` → Finished. JSON `choices[0].delta.content` → Delta. `choices[0].delta.reasoning_content` → Thought. JSON `error` → Error.

`openai_request_body`: `{ "model", "messages", "stream": true }` plus DeepSeek `thinking` / `reasoning_effort` only when `provider_slug == "deepseek"` and thinking_enabled / effort as in `acp-core/src/deepseek.rs`.

`stream_native_chat`: reqwest POST, if status 401/403 return Error("Auth failed"). If not success, Error with status+body. Else read bytes/SSE.

Ollama branch: body `{ "model", "messages", "stream": true }`, parse NDJSON `message.content`.

Do not spawn a child process.

- [ ] **Step 4: Run tests**

Run: `cargo test -p acp-core parse_openai_sse openai_request_json -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/native_chat.rs acp-core/src/lib.rs
git commit -m "feat(acp-core): in-process OpenAI-compatible and Ollama chat stream"
```

---

### Task 5: Runtime + services facade for native prompt, refresh, cancel

**Files:**
- Modify: `acp-core/src/runtime.rs` (or new `acp-core/src/native_runtime.rs` if `runtime.rs` is already huge — prefer new file)
- Modify: `acp-core/src/lib.rs` exports
- Modify: `services/src/lib.rs`

**Interfaces:**
- Consumes: `stream_native_chat`, existing `AcpEvent` / `drain_acp_events`.
- Produces:
  - `pub async fn native_chat_prompt(req: NativeChatRequest) -> Result<(), String>`
  - `pub async fn refresh_provider_models(slug: &str, base_url: &str, api_key: &str) -> Result<Vec<AiModelEntry>, String>`
  - `cancel_acp_prompt` also cancels an in-flight native stream (shared `AtomicBool` / token)

`refresh_provider_models`: GET `{base}/v1/models` with Bearer key. Parse `data[].id`. Ollama: GET `{base}/api/tags` parse `models[].name`. Map to `AiModelEntry { id, label: String::new() }`.

- [ ] **Step 1: Write the failing test**

In `acp-core` tests for refresh JSON:

```rust
#[test]
fn parse_openai_model_list() {
    let json = r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
    let ids = parse_openai_model_list(json).unwrap();
    assert_eq!(ids, ["gpt-4o", "gpt-4o-mini"]);
}

#[test]
fn parse_ollama_tag_list() {
    let json = r#"{"models":[{"name":"qwen3:latest"}]}"#;
    let ids = parse_ollama_tag_list(json).unwrap();
    assert_eq!(ids, ["qwen3:latest"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p acp-core parse_openai_model_list -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Add parsers and `refresh_provider_models`. Wire `native_chat_prompt` to push the same event queue `drain_acp_events` already reads (the queue used by `send_acp_prompt`). If that queue is private in `runtime.rs`, export a `pub(crate) fn push_acp_event(AcpEvent)` from runtime and call it from native_runtime.

Cancel: set the same flag `send_acp_prompt` uses, or a new `NATIVE_CANCEL` that `stream_native_chat` checks between chunks.

Re-export from `services/src/lib.rs` next to `send_acp_prompt`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p acp-core parse_openai_model_list parse_ollama_tag_list -- --nocapture`

Expected: PASS. `cargo check -p services` succeeds.

- [ ] **Step 5: Commit**

```bash
git add acp-core/src/native_chat.rs acp-core/src/native_runtime.rs acp-core/src/runtime.rs acp-core/src/lib.rs services/src/lib.rs
git commit -m "feat(acp): native chat prompt, model refresh, shared cancel"
```

---

### Task 6: Desktop agent panel sends NativeHttp via `native_chat_prompt`

**Files:**
- Modify: `ui/src/screens/workspace/components/agent_panel/requests.rs` (or wherever `send_chat_prompt_request` lives)
- Modify: `ui/src/screens/workspace/components/agent_panel/setup.rs`
- Modify: `ui/src/app_state/mod.rs`

**Interfaces:**
- Consumes: `services::native_chat_prompt`, `APP_UI_SETTINGS().ai_catalog`, `lm_keys`.
- Produces: `send_chat_prompt_request` branches: if `provider_kind(active) == NativeHttp` → native path; if Acp → existing `send_acp_prompt`. `ensure_default_sql_agent_connected` does **not** spawn embedded deepseek/ollama for NativeHttp (no-op connect: set `panel_state.connected = true` with a synthetic `AcpConnectionInfo { agent_name: spec.label }` **or** skip ACP connect and treat native as connected when ActiveModel + key exist).

Pick one and stick to it: **native is connected when `ActiveModel` is NativeHttp and `lm_keys` has a key (Ollama key may be empty)**. `panel_state.connected` true without child. Disconnect on NativeHttp clears connected flag only, no `disconnect_acp_agent`.

- [ ] **Step 1: Write the failing test**

Pure helper in `requests.rs` or `ai_catalog.rs`:

```rust
#[test]
fn native_http_is_ready_without_acp_child() {
    assert!(is_native_http_ready("openai", "sk-test"));
    assert!(is_native_http_ready("ollama", ""));
    assert!(!is_native_http_ready("openai", ""));
    assert!(!is_native_http_ready("acp:opencode", "sk"));
}
```

Put `pub fn is_native_http_ready(provider: &str, api_key: &str) -> bool` in `models/src/ai_catalog.rs` (Ollama slug allows empty key; other NativeHttp require non-empty key; Acp always false).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models is_native_http_ready -- --nocapture`

Expected: FAIL then implement in models (small, allowed as part of this task).

- [ ] **Step 3: Write minimal implementation**

`is_native_http_ready` using `builtin_providers()` kind. Custom ids starting with `custom:` are NativeHttp and need a key.

Change `send_chat_prompt_request` to build `NativeChatRequest` from catalog+keys+thread history (user+agent messages already in `panel_state.messages`) and `spawn` `services::native_chat_prompt`. Drain events as today.

`ensure_default_sql_agent_connected`: if native ready, set connected/status without `connect_embedded_deepseek`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models is_native_http_ready -- --nocapture && cargo clippy -p ui --all-targets -- -D warnings`

Expected: PASS / clippy clean.

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs ui/src/screens/workspace/components/agent_panel/requests.rs ui/src/screens/workspace/components/agent_panel/setup.rs ui/src/app_state/mod.rs
git commit -m "feat(ui): send agent chat through in-process native HTTP"
```

---

### Task 7: Composer model picker

**Files:**
- Modify: `ui/src/screens/workspace/components/agent_panel/composer.rs`
- Modify: `styles/components/_agent-panel.scss`
- Modify: `ui/src/app_state/mod.rs` — `set_active_model(provider: String, model: String)`

**Interfaces:**
- Consumes: `resolve_picker_models`, `builtin_providers`, `ai_catalog`, `set_active_model`.
- Produces: composer footer control showing `"{label} / {model}"`. Click opens a panel (not `document` JS menu) listing Native then Agents. Choosing a NativeHttp model calls `set_active_model` only. Choosing Acp calls existing connect/disconnect then `set_active_model`. Disabled while `busy`.

- [ ] **Step 1: Write the failing test**

Reuse `resolve_picker_models` (already passing). Add:

```rust
#[test]
fn active_model_label_falls_back_to_id() {
    let e = AiModelEntry { id: "gpt-4o".into(), label: String::new() };
    assert_eq!(e.display_label(), "gpt-4o");
    let e = AiModelEntry { id: "gpt-4o".into(), label: "GPT-4o".into() };
    assert_eq!(e.display_label(), "GPT-4o");
}
```

Implement `AiModelEntry::display_label(&self) -> &str` (`if self.label.trim().is_empty() { &self.id } else { &self.label }`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models active_model_label -- --nocapture`

Expected: FAIL then add `display_label`.

- [ ] **Step 3: Write minimal implementation**

`set_active_model` writes `ai_catalog.active` via `update_ui_settings`.

Composer: local `show_picker: Signal<bool>`. Button + menu. For each builtin NativeHttp and each custom_native, a section header and model buttons. ACP slugs in a second section; click runs reconnect as in current Providers connect helpers.

Refresh: `spawn` `services::refresh_provider_models`, merge into `overrides[slug].extra_models` or `custom.models`, skip ids already in builtin. On err `toast_error`.

SCSS: `.agent-panel__model-picker` absolute panel above composer, opaque `var(--color-surface-elevated)`, z-index 30, max-height 240px scroll.

Selects in leftover setup forms: `onchange` if any `oninput` remain on `<select>`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models active_model_label -- --nocapture && cargo clippy -p ui --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs ui/src/screens/workspace/components/agent_panel/composer.rs ui/src/app_state/mod.rs styles/components/_agent-panel.scss
git commit -m "feat(ui): add in-chat provider and model picker"
```

---

### Task 8: Settings Language models CRUD

**Files:**
- Modify: `ui/src/layout/settings_modal.rs` (Language models section; replace per-vendor duplication)

**Interfaces:**
- Consumes: catalog types, `set_active_model`, lm key setters.
- Produces: Advanced section lists each NativeHttp builtin card: enabled, API key, base URL, model rows (id, hide builtin, add extra model), Refresh. Custom block: name, URL, key, models, Add provider, Delete. Setting ActiveModel from a “Use as default” button.

- [ ] **Step 1: Write the failing test**

Catalog helper:

```rust
#[test]
fn delete_custom_resets_active_when_it_was_selected() {
    let mut cat = AiCatalogSettings {
        active: Some(ActiveModel { provider: "custom:1".into(), model: "m".into() }),
        overrides: BTreeMap::new(),
        custom_native: vec![CustomNativeProvider {
            id: "custom:1".into(),
            name: "Mine".into(),
            base_url: "http://localhost:8080".into(),
            models: vec![AiModelEntry { id: "m".into(), label: String::new() }],
        }],
    };
    delete_custom_provider(&mut cat, "custom:1");
    assert!(cat.custom_native.is_empty());
    assert!(cat.active.is_none());
}
```

`pub fn delete_custom_provider(cat: &mut AiCatalogSettings, id: &str)` in `ai_catalog.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models delete_custom_resets -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Implement `delete_custom_provider`. Settings UI: iterate `builtin_providers()` where kind NativeHttp; bind fields to `overrides.entry(slug).or_default()`. Custom: push `CustomNativeProvider { id: format!("custom:{}", Uuid or random u128 hex), ... }`. Use `onchange` on selects. Keys: `lm_keys.insert(id, value)` via `update_ui_settings`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models delete_custom_resets -- --nocapture && cargo clippy -p ui --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs ui/src/layout/settings_modal.rs ui/src/app_state/mod.rs
git commit -m "feat(ui): language model settings catalog and custom providers"
```

---

### Task 9: Picker ACP switch + busy lock + error mapping

**Files:**
- Modify: `ui/src/screens/workspace/components/agent_panel/mod.rs`
- Modify: `ui/src/screens/workspace/components/agent_panel/composer.rs`
- Modify: `acp-core/src/native_chat.rs` (401 mapping if not done)

**Interfaces:**
- Consumes: `disconnect_acp_agent`, `connect_acp_agent`, `is_native_http_ready`.
- Produces: `apply_active_model_change(previous: Option<ActiveModel>, next: ActiveModel)` — if both NativeHttp, only persist. If next is Acp, disconnect then registry/custom connect; on failure restore previous and `toast_error`. Picker `disabled` when `panel_state.busy`. Auth errors already Error messages from Task 4.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn native_to_native_does_not_need_reconnect() {
    assert!(!needs_acp_reconnect("openai", "deepseek"));
    assert!(needs_acp_reconnect("openai", "acp:opencode"));
    assert!(needs_acp_reconnect("acp:opencode", "openai"));
    assert!(!needs_acp_reconnect("openai", "openai"));
}
```

`pub fn needs_acp_reconnect(from: &str, to: &str) -> bool` in `ai_catalog.rs`: true iff `provider_kind(from) != provider_kind(to)` or either is Acp and ids differ.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p models needs_acp_reconnect -- --nocapture`

Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Implement `needs_acp_reconnect` + `provider_kind`. Wire picker on_select. Disable picker while busy.

- [ ] **Step 4: Run tests**

Run: `cargo test -p models needs_acp_reconnect -- --nocapture && cargo clippy -p ui --all-targets -- -D warnings && cargo test -p models settings -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add models/src/ai_catalog.rs ui/src/screens/workspace/components/agent_panel/composer.rs ui/src/screens/workspace/components/agent_panel/mod.rs
git commit -m "feat(ui): reconnect ACP only when leaving native HTTP providers"
```

---

## Self-review (spec coverage)

| Spec requirement | Task |
| --- | --- |
| Catalog types, builtins, custom native | 1 |
| ActiveModel default = picker | 2, 7 |
| Migrate legacy JSON, omit vendor roots on save | 2 |
| Keyring `shovel.lm.<id>`, fallback, old service copy | 3 |
| Native HTTP in-process, SSE/Ollama, events | 4–6 |
| No desktop spawn of acp-agent deepseek/ollama | 6 |
| Picker in composer, Refresh, busy disable | 7, 9 |
| Settings CRUD, hide builtin, extra models | 8 |
| Hot-swap same NativeHttp; ACP reconnect | 9 |
| 401/network/refresh/keyring errors | 4, 9 |
| `onchange` on selects | 7–8 |
| Completions / spec B untouched | constraints |
| `ui` no reqwest | constraints + 5 |

No TBD. Names `ActiveModel`, `resolve_picker_models`, `native_chat_prompt`, `needs_acp_reconnect` are stable across tasks.
