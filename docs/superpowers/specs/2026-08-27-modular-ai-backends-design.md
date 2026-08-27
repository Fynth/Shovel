# Modular AI backends and shared catalog — design

**Date:** 2026-08-27
**Status:** Approved
**Path:** Architectural (catalog data in `models` + protocol backends in `acp-core`)

This document extends
[`2026-08-27-ai-provider-catalog-design.md`](./2026-08-27-ai-provider-catalog-design.md)
(spec A). Spec A shipped a catalog of vendors and in-process native chat. Vendors
are still a giant `const` plus `if slug == "ollama"` / `"deepseek"` in the HTTP
path. SQL autocomplete still talks to DeepSeek and CodeStral from `ui`.

It supersedes two decisions in other specs:

- Spec A non-goal "sharing this catalog with inline SQL completions". Completions
  use the same catalog.
- [`2026-08-27-sql-completion-providers-design.md`](./2026-08-27-sql-completion-providers-design.md)
  non-goal "no CodeStral FIM" and the `SqlCompletionSettings` type. Editor UX in
  that spec (menu, ghost text, variants, keyboard) is unchanged and is not
  implemented here. Provider selection for ghost text is
  `AiCatalogSettings.active_completion`, not a parallel settings struct.

Spec B (autonomous SQL agent) is still out of scope.

## Problem

Almost every builtin vendor speaks OpenAI chat completions. The code treats them
as if each slug were a protocol. That shows up as:

- `builtin_providers()` mixing 40 vendor rows with no `backend` or `group` field
- `provider_group()` matching slugs
- `native_chat.rs` / `native_runtime.rs` branching on `"ollama"` and `"deepseek"`
- `ui/src/completion.rs` owning HTTP, URLs, and a hardcoded DeepSeek→CodeStral
  fallback

Adding Groq-like hosts means more rows. Adding Anthropic or real FIM should mean
a new backend, not another slug `if`.

## Goal

- A vendor is data: slug, label, URL, group, default models, which backend,
  whether thinking fields are sent.
- A backend is code: OpenAI-compatible HTTP, Ollama, Mistral FIM. Runtime
  dispatches on `backend_id`, never on vendor slug.
- Chat and autocomplete share the catalog and the backends. They do not share
  the selected model.
- `ui` does not call provider HTTP. Autocomplete goes through `services`.
- Custom OpenAI-compatible providers stay. They pick a backend (default
  OpenAI-compat).

## Non-goals

- One Rust module or crate per vendor.
- `inventory` / plugin crates for backends.
- JSON/TOML catalog file.
- New workspace crate (`ai`). Backends live in `acp-core` next to native chat.
- Anthropic, Gemini native, Bedrock as backends in this change (the enum is
  ready for them later).
- ACP as an `AiBackend`. OpenCode/Codex stay on the child transport. Autocomplete
  never uses ACP.
- Editor completion menu, ghost-text layout, variant cycling (other spec).
- Fake FIM by sending a chat prompt to a FIM-only host. OpenAI-compat `complete`
  uses chat completions with a SQL prompt because that is the wire format those
  hosts already speak. Mistral FIM uses `/v1/fim/completions`.

## Approach

Chosen: **catalog as a const table, backends as a small trait, two active
selections**.

Rejected:

- Per-vendor modules. Groq and OpenRouter would be copies of the same HTTP.
- Bundled JSON catalog. This is a compiled desktop app; parse errors would move
  to runtime for no shipping benefit.
- `inventory` like DB drivers. Three backends do not need self-registration.
- One `ActiveModel` for both chat and SQL. User wants Grok in chat and Codestral
  in the editor at the same time.

## Architecture

Layer rules from `ARCHITECTURE.md` stay.

| Piece | Crate | May import |
| --- | --- | --- |
| Catalog types, specs, migration | `models` | none of ui/acp |
| Keyring `shovel.lm.<provider_id>` | `storage` | `models` |
| `AiBackend` trait + three impls, chat/complete HTTP | `acp-core` | `models` |
| Facade `native_chat_prompt` / `complete_sql` / `refresh_provider_models` | `services` | acp-core, storage |
| Pickers, settings cards, editor trigger | `ui` | `models`, `services` only |

`ui` must not call `reqwest` against provider URLs. After this change
`ui/src/completion.rs` has no vendor URL and no `reqwest` import.

ACP remains a catalog kind, not a backend in the trait. Switching to or from
`acp:*` still reconnects the child. NativeHttp→NativeHttp does not.

```text
BuiltinProviderSpec.backend
        │
        ▼
  backend(id) -> &'static dyn AiBackend
        │
        ├── chat        -> AcpEvent stream (existing bus)
        ├── complete    -> CompletionToken stream
        └── list_models -> Vec<AiModelEntry>
```

## Components

### Backend id and capabilities (`models`)

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

`AiProviderKind::NativeHttp` vs `Acp` is derived: a spec with `backend: Some(_)`
is NativeHttp; `backend: None` is Acp. Do not store both `kind` and `backend`
as independent fields.

### Builtin spec (`models`)

```rust
pub struct BuiltinProviderSpec {
    pub slug: &'static str,
    pub label: &'static str,
    pub backend: Option<AiBackendId>, // None => Acp
    pub group: AiProviderGroup,
    pub default_base_url: &'static str,
    pub builtin_models: &'static [(&'static str, &'static str)],
    pub supports_thinking: bool,
}
```

`provider_group(slug)` is deleted. Callers read `spec.group`. Unknown /
`custom:*` group is `Cloud`. `acp:*` without a spec is `Agent`.

`supports_model_refresh` is `backend_capabilities(id).list_models`. Do not keep
a parallel bool on the spec.

`supports_thinking` is true only for `deepseek`. Chat request bodies send
`thinking` / `reasoning_effort` only when this flag is set, never when
`slug == "deepseek"`.

New builtin row:

| Field | Value |
| --- | --- |
| slug | `codestral` |
| label | Codestral |
| backend | `MistralFim` |
| group | Cloud |
| default_base_url | `https://codestral.mistral.ai` |
| builtin_models | `codestral-latest` |
| supports_thinking | false |

Existing `mistral` stays OpenAI-compat chat at `https://api.mistral.ai`. It is
not FIM.

### Catalog settings (`models`)

```rust
pub struct AiCatalogSettings {
    pub active: Option<ActiveModel>,
    pub active_completion: Option<ActiveModel>,
    pub overrides: BTreeMap<String, AiProviderOverride>,
    pub custom_native: Vec<CustomNativeProvider>,
}

pub struct CustomNativeProvider {
    pub id: String, // custom:<uuid>
    pub name: String,
    pub base_url: String,
    pub models: Vec<AiModelEntry>,
    #[serde(default = "default_custom_backend")]
    pub backend: AiBackendId, // OpenAiCompat
}
```

`ActiveModel` is unchanged `{ provider, model }`. Chat reads `active`. SQL
ghost text reads `active_completion`. Pickers do not write each other's slot.

`delete_custom_provider` clears `active` and/or `active_completion` when the
deleted id is in that slot.

Do not add `SqlCompletionSettings`. If that type appears in a later editor
spec, it must alias these fields or be dropped.

### Trait (`acp-core`)

Backends live under `acp-core/src/backends/`: `mod.rs`, `openai.rs`,
`ollama.rs`, `mistral_fim.rs`. Dispatch:

```rust
pub fn backend(id: AiBackendId) -> &'static dyn AiBackend;
```

A `match` on three variants is the registry. No `inventory`.

```rust
pub trait AiBackend: Send + Sync {
    fn id(&self) -> AiBackendId;
    fn capabilities(&self) -> AiCapabilities;

    fn chat_url(&self, base: &str) -> Result<String, String>;
    fn complete_url(&self, base: &str) -> Result<String, String>;
    fn models_url(&self, base: &str) -> Result<String, String>;

    fn chat_body(&self, req: &NativeChatRequest) -> Result<Value, String>;
    fn complete_body(&self, req: &CompleteRequest) -> Result<Value, String>;

    fn parse_chat(&self, payload: &str) -> Vec<NativeChatEvent>;
    fn parse_complete(&self, payload: &str) -> Vec<CompletionToken>;
    fn parse_models(&self, json: &str) -> Result<Vec<AiModelEntry>, String>;
}
```

`chat_url` / `complete_url` / `models_url` return `Err` when the capability is
missing. HTTP send/retry/cancel stays in `native_chat.rs` / a completion
runner; the trait does not own `reqwest`.

`NativeChatRequest` drops slug-based dispatch. It carries the backend and the
thinking flag the builder already resolved from the spec:

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

`provider_slug` is removed. `chat_body` sends `thinking` /
`reasoning_effort` only when `supports_thinking` is true.

`CompleteRequest`:

```rust
pub struct CompleteRequest {
    pub backend: AiBackendId,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub prefix: String,
    pub suffix: Option<String>,
    pub schema_context: String,
}
```

Wire behaviour:

| Backend | chat | complete | list_models |
| --- | --- | --- | --- |
| OpenAiCompat | `POST {base}/v1/chat/completions` SSE | same URL, SQL system+user prompt, `max_tokens: 100`, `temperature: 0.1`, `stop: ["\n\n", ";", "```"]`, ignore `reasoning_content` | `GET {base}/v1/models` |
| Ollama | `POST {base}/api/chat` NDJSON | same chat URL + SQL prompt | `GET {base}/api/tags` |
| MistralFim | `Err` | `POST {base}/v1/fim/completions`, one-shot JSON, yield one `Text` then `Done` | `Err` |

URL normalize stays `normalize_native_chat_url`. OpenAI path rules stay: if
base already contains `chat/completions`, do not append it; if it ends with
`/v1`, append `/chat/completions` only. FIM: `{normalized}/v1/fim/completions`
unless base already contains `fim/completions`.

SQL prompt for OpenAI-compat and Ollama complete is the current DeepSeek
prompt in `ui/src/completion.rs` (schema, prefix, `[CURSOR]`, suffix, raw SQL
only). It lives next to the OpenAI backend, not in `ui`.

### Facade (`services`)

```rust
pub async fn native_chat_prompt(req: NativeChatRequest) -> Result<(), String>;
pub fn complete_sql(req: CompleteRequest) -> UnboundedReceiver<CompletionToken>;
pub async fn refresh_provider_models(
    backend: AiBackendId,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<AiModelEntry>, String>;
```

`refresh_provider_models` calls `backend(id).models_url` / `parse_models`.
The UI resolves `AiBackendId` from the spec or custom row before the call.
Slug `"ollama"` is not an argument.

`complete_sql` does not read or write `NATIVE_CHAT_CANCEL`. Dropping the
receiver aborts. Timeout 15s. Chat timeout stays 120s.

`ui` constructs `CompleteRequest` from `active_completion` + key + URL via
catalog helpers, then calls `services::complete_sql`. No fallback chain.

### UI

- Chat picker writes `active`. Completion picker / settings write
  `active_completion`.
- Completion picker lists enabled providers whose backend has
  `complete: true` (OpenAI-compat, Ollama, Codestral, custom OpenAI-compat).
  No `acp:*`.
- Chat picker unchanged besides using `spec.group` instead of
  `provider_group(slug)`.
- Settings: Codestral is a catalog card (key, enable, models), not a
  one-off section bound to `CodeStralSettings`. Legacy `codestral` /
  `deepseek` structs stay on `AppUiSettings` so old JSON loads. Runtime
  reads the catalog only. This change does not delete those structs.
- `CompletionService` stays as a thin wrapper so `sql_editor.rs` can keep
  calling `stream_completion`. It builds `CompleteRequest` from
  `active_completion` and forwards to `services::complete_sql`.
  DeepSeek→CodeStral fallback is deleted.

## Data flow

**Startup.** Load JSON → migrate → hydrate keys. If `active` is `None`, first
enabled NativeHttp with credentials and a model, else `None`. If
`active_completion` is `None`, first enabled provider with `complete` and
credentials (after migration this is codestral or deepseek when those were
enabled).

**Legacy migration** (Deserialize of `AppUiSettings` or the existing catalog
migrate path):

- Existing catalog migrate for chat `active` stays.
- If `active_completion` is `None` and `codestral.enabled` and model
  non-empty: `active_completion = { provider: "codestral", model }`. Copy
  key into `shovel.lm.codestral` if that slot is empty (legacy
  `shovel.codestral` already has a load path).
- Else if `deepseek.enabled` and model non-empty: `active_completion =
  { provider: "deepseek", model }`.
- Set `overrides["codestral"].enabled` from `codestral.enabled`.

**Chat send.** NativeHttp: spec → `backend(id).chat_*` → existing `AcpEvent`
bus. Acp: `send_acp_prompt`. Body model is current `active.model`.

**Autocomplete.** Editor calls `services::complete_sql` with
`active_completion`. Missing selection or backend without `complete`: empty
stream, no toast. One provider only.

**Picker.** Chat: persist `active`; ACP reconnect only when
`needs_acp_reconnect`. Completion: persist `active_completion` only.

**Refresh.** Merge ids into `extra_models` / `custom.models`, skip builtin
ids. Failure: toast, list unchanged.

**Custom add.** `custom:<uuid>`, `backend: OpenAiCompat`, empty models.
**Custom delete.** Drop keyring entry; clear either active slot that pointed
at it.

## Error handling

| Case | Behavior |
| --- | --- |
| No key / empty `active` on Send | Toast; no request |
| Empty `active_completion` on keystroke | Silent empty stream |
| 401 / 403 chat | Stream stops; one Error line in thread; key kept |
| 401 / 403 complete | One `CompletionToken::Error`; key kept |
| Network / timeout / 5xx chat | Error line; retry is another Send |
| Network / timeout / 5xx complete | `Error` token; no fallback provider |
| Refresh fail | Toast; extra_models unchanged |
| Keyring down | Fallback file; one warning toast per session; JSON still saves |
| ACP launch fail after chat picker switch | Revert `active` to previous NativeHttp if any; toast; thread intact; `active_completion` untouched |
| Chat busy | Chat picker disabled |
| Complete vs chat | Independent; complete does not set chat busy |
| `complete` / `list_models` on a backend that lacks it | `Err` from the trait; UI must not offer that action |
| Corrupt catalog JSON | Builtin defaults; do not delete keyring keys |
| Custom URL | Normalize once; never `.../v1/v1/chat/completions` |

## Testing

No live vendor keys. No full Dioxus chat in CI.

**models**

- Specs carry `backend` and `group`. `codestral` is `MistralFim`.
- `provider_kind` / capabilities come from `backend`, not slug tables.
- `provider_group` function is gone; `spec.group` matches current
  classification (subscription slugs, `ollama` Local, `acp:*` Agent, else
  Cloud).
- `supports_thinking` is true only for `deepseek`.
- Legacy codestral/deepseek JSON fills `active_completion`.
- Secrets still skipped in serialize.
- Hide+extra merge unchanged.
- Deleting a custom provider clears both slots when it was selected.
- URL normalize does not double `/v1`.

**acp-core**

- OpenAI SSE fixture → chat events.
- Same fixture through `complete` yields `content` only, not reasoning.
- Ollama uses `/api/chat` and `/api/tags`.
- MistralFim POST path ends with `/v1/fim/completions`; `chat_url` errors.
- `chat_body` includes `thinking` only when `supports_thinking` is true on
  the request. A unit test sets the flag on a request whose backend is
  OpenAiCompat without involving the deepseek slug.
- Second chat prompt after model change sends the new model id.
- 401 → Error, history preserved.
- Cancel aborts chat stream; complete does not flip `NATIVE_CHAT_CANCEL`.
- `parse_models` for OpenAI list JSON and Ollama tags JSON.

**services**

- Facade smoke exports `complete_sql`, `native_chat_prompt`,
  `refresh_provider_models`.

**ui**

- `ui/src/completion.rs` does not contain `https://` vendor URLs and does
  not import `reqwest`.
- Completion picker helper returns only enabled + `complete`.
- Autocomplete request builder reads `active_completion`, not `active`.

ACP OpenCode/Codex still launch via registry (existing tests). CI does not
hit vendor networks.

## Files

- `models/src/ai_catalog.rs` — spec fields, backend id, capabilities,
  `active_completion`, custom `backend`, delete-both-slots, drop
  `provider_group` slug match
- `models/src/settings.rs` / `settings_roundtrip.rs` — migrate
  `active_completion` from codestral/deepseek
- `acp-core/src/backends/{mod,openai,ollama,mistral_fim}.rs` — trait + impls
- `acp-core/src/native_chat.rs` — slug branches removed; call trait
- `acp-core/src/native_runtime.rs` — refresh via trait
- `acp-core/src/native_complete.rs` — `complete_sql` runner, 15s timeout,
  no `NATIVE_CHAT_CANCEL`
- `acp-core/src/lib.rs`, `acp/src/lib.rs`, `services/src/lib.rs` — re-exports
- `ui/src/completion.rs` — strip `reqwest` and vendor URLs; wrap
  `services::complete_sql`
- `ui` agent-panel catalog grouping and settings Codestral card
- `ui/src/app_state/` — `set_active_completion`
- `storage/src/settings.rs` — on catalog load, if `shovel.lm.codestral` is
  empty, copy from legacy `shovel.codestral` (same pattern as other LM
  keys)

## Risks

- `ui` still compiling `reqwest` for other reasons is fine; completion must
  not be one of them.
- Codestral today ignores a configurable base URL and posts to a const. The
  spec URL is the default; override.base_url must work.
- OpenAI-compat `complete` is chat-with-prompt. Do not send FIM bodies to
  Groq/OpenRouter. Do not send chat-completion bodies to Codestral.
- `NativeChatRequest.supports_thinking` is set by the UI/services builder
  from `spec.supports_thinking`. `acp-core` never looks up a slug for
  thinking or URLs.

## Open decisions (closed in brainstorming)

- Vendors are data, protocols are backends.
- Chat and autocomplete share catalog and backends.
- Two selections: `active` and `active_completion`.
- Approach A: const catalog, trait in `acp-core`, no extra crate.
)
