# Native AI provider catalog and in-chat model picker — design (spec A)

**Date:** 2026-08-27
**Status:** Draft (awaiting user review)
**Path:** Architectural (catalog in `models` + native HTTP chat in `acp-core` + picker UI)

Spec B (autonomous SQL agent, schema index, DB Q&A) is a follow-up. This
document does not specify it.

## Problem

AI in Shovel is split across one-off settings structs (`DeepSeekSettings`,
`OllamaSettings`, `OpenAiCompatSettings` for OpenAI/Groq/OpenRouter/xAI/Mistral)
and ACP child processes (`shovel acp-agent deepseek|ollama`) that bake the model
into launch argv. The agent panel has a Providers screen, not a Zed-style picker
in the composer. Changing model requires reconnect. Selects in the panel used
`oninput`, which WebKitGTK does not fire. There is no user-defined
OpenAI-compatible provider, no per-provider model CRUD, no Refresh from
`/v1/models`.

URL-based providers do not need ACP. ACP is the right transport for OpenCode,
Codex, and a custom stdio binary.

## Goal

- Built-in URL providers feel native: in-process HTTP, model list, default
  model, picker in the chat composer.
- User can add/edit/delete custom OpenAI-compatible providers and models.
- One persisted `ActiveModel { provider, model }` is both the default and the
  chat selection. Changing it in chat writes settings.
- Same-provider model change does not restart anything. Switching to or from an
  ACP agent reconnects that child; the chat thread stays.
- Non-URL providers stay on ACP.

Out of scope (spec B or later): agent-run SQL without the editor, schema
indexing, answering arbitrary data questions via tools, Anthropic/Google/Bedrock
APIs, sharing this catalog with inline SQL completions (CodeStral/DeepSeek
completion path stays as-is).

## Approach

Chosen: **native HTTP for URL providers, ACP only for non-URL agents**.

Rejected:

- Keep ACP children for DeepSeek/Ollama and poke the model through a sidecar
  file. Two sources of truth, races, hard to debug.
- Restart the ACP child on every model change. Conflicts with the approved
  hot-swap rule.

## Architecture

Layer rules from `ARCHITECTURE.md` stay:

| Piece | Crate | May import |
| --- | --- | --- |
| Catalog types, `ActiveModel`, migration | `models` | none of ui/acp |
| Keyring `shovel.lm.<provider_id>`, JSON without secrets | `storage` | `models` |
| Native chat HTTP + stream → existing `AcpEvent`s | `acp-core` | `models` |
| Facade `native_chat_prompt` / `refresh_provider_models` | `services` | acp-core, storage |
| Composer picker, Settings → Language models | `ui` | `models`, `services` only |

`ui` must not call `reqwest` or provider URLs.

### Provider kinds

```text
NativeHttp  OpenAI, Groq, DeepSeek, OpenRouter, xAI, Mistral, Ollama, custom:*
Acp         OpenCode, Codex, custom stdio (existing launch.command / args / cwd)
```

`AiProviderId` is a string: builtins are stable slugs (`deepseek`, `openai`,
`ollama`, `acp:opencode`, `acp:codex`). Custom native ids are `custom:<uuid>`.

## Components

### Catalog (`models`)

```rust
pub struct ActiveModel {
    pub provider: String, // AiProviderId
    pub model: String,
}

pub struct AiModelEntry {
    pub id: String,
    pub label: String, // empty ⇒ show id
}

pub struct AiProviderOverride {
    pub enabled: bool,
    pub base_url: String,          // empty ⇒ builtin default
    pub extra_models: Vec<AiModelEntry>,
    pub hidden_builtin_ids: Vec<String>,
}

pub struct CustomNativeProvider {
    pub id: String, // custom:<uuid>
    pub name: String,
    pub base_url: String,
    pub models: Vec<AiModelEntry>,
}

pub struct AiCatalogSettings {
    pub active: Option<ActiveModel>,
    pub overrides: BTreeMap<String, AiProviderOverride>, // keyed by builtin slug
    pub custom_native: Vec<CustomNativeProvider>,
}
```

Builtin specs (not persisted) live as `const` data: slug, label, default base
URL, curated `AiModelEntry` list, `supports_model_refresh` (true for OpenAI-compat
HTTP and Ollama).

`AppUiSettings` gains `ai_catalog: AiCatalogSettings`. Legacy fields
`deepseek`, `openai`, `groq`, `openrouter`, `xai`, `mistral`, `ollama` deserialize
into the catalog in `AppUiSettings`'s `Deserialize` impl (or a
`fn migrate_legacy_ai_fields` called from that impl). They are not written
back: serialize only `ai_catalog`. After migration:

- `ActiveModel` from the first legacy struct that was `enabled` with a model,
  else `None`.
- Override.base_url and extra/hidden filled from the old model string if it was
  not in the builtin list (treat as extra model).
- API keys stay in keyring under the new `shovel.lm.<slug>` service. On
  migration, copy from existing `shovel.deepseek` / `shovel.openai` / … services
  if the new key is empty.

Ollama `thinking` / DeepSeek `reasoning_effort` / `thinking_enabled` remain on
the DeepSeek/Ollama override as optional fields **only if** we keep them on
`AiProviderOverride` as `extra: BTreeMap<String, String>` or dedicated optional
fields on override for `deepseek` only. Decision: keep
`reasoning_effort` and `thinking_enabled` as optional fields on
`AiProviderOverride` (ignored by providers that do not send them). Empty /
default means off / `"medium"`.

### Secrets (`storage`)

- Service: `shovel.lm.<provider_id>` account `default`.
- Load: keyring, else fallback file (existing helper).
- Save: keyring, on failure write fallback and return a **warning** string, not
  a hard error that blocks JSON settings save. JSON catalog save always
  proceeds. UI toasts the warning once per session if fallback was used.
- Delete custom provider: delete keyring entry and fallback.

### Native chat (`acp-core`)

Trait `NativeChatBackend`: `chat_url`, `build_request`, `parse_sse_or_json_stream`.

Two backends:

- `OpenAiCompatBackend` — `POST {normalize(base)}/v1/chat/completions` unless
  `base` already ends with `/chat/completions`.
- `OllamaBackend` — existing Ollama chat path (`/api/chat` after normalizing
  the historic `/api` suffix).

Session object holds conversation history for the **current Shovel chat
thread id**, maps stream deltas to `AcpEvent::Message { kind: Agent, text }`,
`PromptStarted`, `PromptFinished`, `Error`. Cancel uses one facade `services::cancel_acp_prompt` that cancels whichever
backend is live (native stream or ACP child).

The child binaries `shovel acp-agent deepseek` and `acp-agent ollama` remain
in the tree for CLI/headless use; the **desktop chat panel does not spawn
them** for NativeHttp providers.

ACP providers: unchanged `connect_acp_agent` / registry install.

### UI

Composer (connected or native-ready): control `Provider / model` opening a
menu. Sections: Native, Agents. Models from
`resolve_picker_models(spec, override, last_fetch)`. Active row checkmark.
Refresh item when `supports_model_refresh`. Disabled while `busy`.

Settings → Advanced: builtin cards (key, URL, model list, add model, hide
builtin), Custom providers CRUD, display of current `ActiveModel`.

Select elements use `onchange`, not `oninput` (WebKitGTK).

`ui` calls only `services::*`.

## Data flow

**Startup.** Load JSON → migrate → hydrate keys into memory (not back into
JSON). If `ActiveModel` is `None`, pick first NativeHttp with a non-empty key
and a model, else Ollama if a model id is set, else leave `None` and the
composer shows an empty picker.

**Send (NativeHttp).** Read `ActiveModel`, key, URL →
`services::native_chat_prompt(thread, prompt, active)`. Model id in the HTTP
body is the current active model, not the model the thread started with.

**Send (Acp).** Existing ACP prompt. Picker still shows the ACP agent; its
models list is empty or a single placeholder unless the agent later grows a
model list (not in spec A).

**Picker change.** Write `ActiveModel`, persist. Same NativeHttp provider:
no process work. Any transition that crosses ACP (Native↔Acp or Acp↔Acp):
`disconnect_acp_agent` then connect if the target is Acp. Thread messages
untouched.

**Refresh.** `services::refresh_provider_models(provider_id)` → merge into
`extra_models`. Builtin ids are not duplicated. Failure: toast, list
unchanged.

**Add custom native.** New `custom:<uuid>`, empty models, user adds ids or
Refresh. **Delete custom.** If it was `ActiveModel.provider`, set
`ActiveModel` to the first remaining viable builtin (key + model) or `None`.

## Error handling

| Case | Behavior |
| --- | --- |
| No key / empty ActiveModel | Picker opens; Send no-ops with a toast. No surprise ACP connect. |
| 401 / 403 | Stream stops; one Error chat line; status Auth failed; key kept. |
| Network / timeout / 5xx | Error line; short status; retry is another Send. |
| Refresh fail | Toast; catalog unchanged. |
| Keyring down | Fallback store; one toast per session. |
| ACP launch fail after switch | Revert `ActiveModel` to previous NativeHttp if any; toast; thread intact. |
| Busy | Picker disabled until PromptFinished/Error. |
| Custom URL | Normalize once; never emit `.../v1/v1/chat/completions`. |
| Corrupt catalog JSON | Builtins defaults; do not delete keyring keys. |

## Testing

- **models:** migration from legacy `deepseek`/`openai`/… JSON; secrets never
  in serialize; hide+extra merge; deleting the active custom provider resets
  ActiveModel; URL normalize cases.
- **acp-core:** mock HTTP stream concatenates to UI events; second prompt after
  model change sends the new model id; 401 → Error and history preserved;
  cancel aborts; Ollama backend uses its path.
- **storage:** save/load override without keys; keyring error uses fallback.
- **pure UI helper:** `resolve_picker_models` order and dedupe; no full
  Dioxus chat in CI.
- **ACP regression:** OpenCode/Codex still launch via registry (existing
  tests / smoke if present). Do not require a live OpenAI key in CI.

## File touch list (implementation, not this PR)

- `models/src/settings.rs` (catalog types, migration, serde tests)
- `storage/src/settings.rs` (lm keyring by provider id)
- `acp-core` native backend + event mapping
- `services/src/lib.rs` re-exports
- `ui` composer picker, settings cards, `onchange` on selects
- `ui` agent panel: stop spawning embedded deepseek/ollama for desktop chat

## Risks

- In-process native chat must reuse the same `AcpEvent` shapes or the panel
  will fork. Prefer adapting the existing event bus over a second chat path.
- Ollama base URL in the wild is mixed (`http://localhost:11434` vs
  `.../api`). Normalization must stay compatible with current
  `OllamaSettings.base_url` default.
- Migration must not drop API keys sitting in old keyring service names.
