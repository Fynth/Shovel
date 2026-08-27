use dioxus::prelude::*;
use models::{
    AcpPanelState,
    ActiveModel,
    AiModelEntry,
    AiProviderKind,
    AppUiSettings,
    builtin_providers,
    resolve_picker_models,
};

use super::{
    prompt::{active_editor_error, active_editor_sql},
    requests::{
        send_chat_prompt_request,
        send_sql_error_fix_request,
        send_sql_explanation_request,
        send_sql_generation_request,
        send_sql_plan_request,
    },
    setup::{connect_registry_agent, ensure_opencode_connected},
};

use crate::screens::workspace::tab_store::TabStore;

#[component]
pub(super) fn AgentComposer(
    panel_state: Signal<AcpPanelState>,
    store: TabStore,
    chat_revision: Signal<u64>,
    allow_agent_db_read: Signal<bool>,
    allow_agent_read_sql_run: Signal<bool>,
    allow_agent_write_sql_run: Signal<bool>,
    allow_agent_tool_run: Signal<bool>,
    busy: bool,
    connection_label: String,
    reset_key: String,
) -> Element {
    let mut prompt_draft = use_signal(String::new);
    let mut prompt_reset_revision = use_signal(|| 0_u64);
    let reset_effect_key = reset_key.clone();

    use_effect(move || {
        let _ = reset_effect_key.as_str();
        prompt_draft.set(String::new());
    });

    let prompt_is_empty = prompt_draft().trim().is_empty();
    let active_sql = active_editor_sql(store, store.active_tab_id());
    let has_active_sql = active_sql.is_some();
    let has_explainable_sql = active_sql
        .as_deref()
        .is_some_and(services::is_read_only_sql);
    let has_active_error = active_editor_error(store, store.active_tab_id()).is_some();
    let enter_chat_label = connection_label.clone();
    let generate_sql_label = connection_label.clone();
    let chat_label = connection_label.clone();
    let explain_plan_label = connection_label.clone();
    let explain_sql_label = connection_label.clone();
    let fix_sql_label = connection_label.clone();
    let prompt_textarea_key = format!("{reset_key}-{}", prompt_reset_revision());

    // Focus the composer textarea when the workspace dispatcher bumps
    // the global focus-request counter (Ctrl+Shift+M). Mirrors the SQL
    // editor's focus wiring.
    let focus_target_id = prompt_textarea_key.clone();
    use_effect(move || {
        let _ = crate::app_state::APP_FOCUS_AGENT_COMPOSER_REQUEST();
        let _ = document::eval(&format!(
            r#"
            (() => {{
                const el = document.getElementById({id:?});
                if (el) {{
                    el.focus();
                }}
            }})()
            "#,
            id = focus_target_id
        ));
    });

    rsx! {
        div { class: "agent-panel__composer",
            textarea {
                key: "{prompt_textarea_key}",
                class: "input agent-panel__prompt",
                rows: 1,
                value: "{prompt_draft}",
                placeholder: "Ask the agent…",
                oninput: move |event| prompt_draft.set(event.value()),
                onkeydown: move |event| {
                    // Send on bare Enter (chat-style) or Ctrl+Enter
                    // (editor-style). Shift+Enter inserts a newline.
                    if event.key() != Key::Enter
                        || event.modifiers().contains(Modifiers::SHIFT)
                    {
                        return;
                    }
                    event.prevent_default();
                    let prompt = prompt_draft();
                    if prompt.trim().is_empty() || panel_state().busy {
                        return;
                    }
                    prompt_draft.set(String::new());
                    prompt_reset_revision += 1;
                    send_chat_prompt_request(
                        panel_state,
                        store,
                        store.active_tab_id(),
                        enter_chat_label.clone(),
                        chat_revision,
                        allow_agent_db_read(),
                        prompt,
                        prompt_draft,
                    );
                }
            }
            AgentModelPicker {
                panel_state,
                chat_revision,
                busy,
            }
            div { class: "agent-panel__composer-actions",
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !allow_agent_read_sql_run() || !has_explainable_sql,
                    onclick: move |_| {
                        send_sql_plan_request(
                            panel_state,
                            store,
                            store.active_tab_id(),
                            explain_plan_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            allow_agent_read_sql_run(),
                        );
                    },
                    title: "Explain the execution plan of the active read-only SQL",
                    "Explain Plan"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !has_active_sql,
                    onclick: move |_| {
                        send_sql_explanation_request(
                            panel_state,
                            store,
                            store.active_tab_id(),
                            explain_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                        );
                    },
                    title: "Explain the active SQL with the agent",
                    "Explain SQL"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || !has_active_error,
                    onclick: move |_| {
                        send_sql_error_fix_request(
                            panel_state,
                            store,
                            store.active_tab_id(),
                            fix_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                        );
                    },
                    title: "Ask the agent to fix the latest SQL error",
                    "Fix SQL Error"
                }
                button {
                    class: "button button--ghost button--small",
                    disabled: busy || prompt_is_empty,
                    onclick: move |_| {
                        let prompt = prompt_draft();
                        if prompt.trim().is_empty() || panel_state().busy {
                            return;
                        }
                        prompt_draft.set(String::new());
                        prompt_reset_revision += 1;
                        send_sql_generation_request(
                            panel_state,
                            store,
                            store.active_tab_id(),
                            generate_sql_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            prompt,
                            Some(prompt_draft),
                            true,
                        );
                    },
                    title: "Generate SQL only and insert it into the active editor",
                    "Generate SQL"
                }
                button {
                    class: "button button--primary button--small",
                    disabled: busy || prompt_is_empty,
                    onclick: move |_| {
                        let prompt = prompt_draft();
                        if prompt.trim().is_empty() || panel_state().busy {
                            return;
                        }
                        prompt_draft.set(String::new());
                        prompt_reset_revision += 1;
                        send_chat_prompt_request(
                            panel_state,
                            store,
                            store.active_tab_id(),
                            chat_label.clone(),
                            chat_revision,
                            allow_agent_db_read(),
                            prompt,
                            prompt_draft,
                        );
                    },
                    title: if prompt_is_empty {
                        "Type a prompt to send to the agent"
                    } else {
                        "Send prompt to the agent (Enter)"
                    },
                    if busy {
                        span { class: "agent-panel__streaming-caret", aria_hidden: "true" }
                        " Sending…"
                    } else {
                        "Send"
                    }
                }
            }
        }
    }
}

#[component]
fn AgentModelPicker(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    busy: bool,
) -> Element {
    let mut show_picker = use_signal(|| false);
    let settings = crate::app_state::APP_UI_SETTINGS();
    let trigger_label = picker_trigger_label(&settings);
    let native_sections = native_picker_sections(&settings);
    let acp_agents = acp_picker_agents();
    let active = settings.ai_catalog.active.clone();
    let trigger_class = if show_picker() {
        "button button--ghost button--small button--active agent-panel__model-trigger"
    } else {
        "button button--ghost button--small agent-panel__model-trigger"
    };

    rsx! {
        div { class: "agent-panel__composer-footer",
            button {
                class: trigger_class,
                disabled: busy,
                title: "Select language model",
                "aria-expanded": "{show_picker()}",
                onclick: move |_| show_picker.set(!show_picker()),
                {trigger_label}
            }
            if show_picker() {
                div {
                    class: "agent-panel__model-picker",
                    onclick: move |event| event.stop_propagation(),
                    p { class: "agent-panel__model-picker-group-title", "Native" }
                    for section in native_sections {
                        {
                            let section_slug = section.slug.clone();
                            let refresh_slug = section.slug.clone();
                            rsx! {
                                div { class: "agent-panel__model-picker-section",
                                    div { class: "agent-panel__model-picker-header",
                                        span { {section.label} }
                                        if section.supports_refresh {
                                            button {
                                                class: "button button--ghost button--small",
                                                disabled: busy,
                                                onclick: move |event| {
                                                    event.stop_propagation();
                                                    refresh_picker_models(refresh_slug.clone());
                                                },
                                                "Refresh"
                                            }
                                        }
                                    }
                                    for model in section.models {
                                        {
                                            let provider = section_slug.clone();
                                            let model_id = model.id.clone();
                                            let is_active = active.as_ref().is_some_and(|current| {
                                                current.provider == provider && current.model == model_id
                                            });
                                            let item_class = if is_active {
                                                "button button--ghost button--small agent-panel__model-picker-item agent-panel__model-picker-item--active"
                                            } else {
                                                "button button--ghost button--small agent-panel__model-picker-item"
                                            };
                                            rsx! {
                                                button {
                                                    class: item_class,
                                                    disabled: busy,
                                                    onclick: move |_| {
                                                        crate::app_state::set_active_model(
                                                            provider.clone(),
                                                            model_id.clone(),
                                                        );
                                                        show_picker.set(false);
                                                    },
                                                    span {
                                                        if is_active {
                                                            "✓ "
                                                        }
                                                        {model.display_label().to_string()}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    p { class: "agent-panel__model-picker-group-title", "Agents" }
                    for (slug, label) in acp_agents {
                        {
                            let is_active = active
                                .as_ref()
                                .is_some_and(|current| current.provider == slug);
                            let item_class = if is_active {
                                "button button--ghost button--small agent-panel__model-picker-item agent-panel__model-picker-item--active"
                            } else {
                                "button button--ghost button--small agent-panel__model-picker-item"
                            };
                            rsx! {
                                button {
                                    class: item_class,
                                    disabled: busy,
                                    onclick: move |_| {
                                        connect_acp_from_picker(
                                            panel_state,
                                            chat_revision,
                                            slug.clone(),
                                        );
                                        show_picker.set(false);
                                    },
                                    span {
                                        if is_active {
                                            "✓ "
                                        }
                                        {label}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct NativePickerSection {
    slug: String,
    label: String,
    supports_refresh: bool,
    models: Vec<AiModelEntry>,
}

fn picker_trigger_label(settings: &AppUiSettings) -> String {
    let Some(active) = settings.ai_catalog.active.as_ref() else {
        return "Select model".to_string();
    };
    let label = provider_display_label(settings, &active.provider);
    let model = model_display_label(settings, active);
    format!("{label} / {model}")
}

fn provider_display_label(settings: &AppUiSettings, provider: &str) -> String {
    if let Some(spec) = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
    {
        return spec.label.to_string();
    }
    settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
        .map(|custom| {
            if custom.name.trim().is_empty() {
                custom.id.clone()
            } else {
                custom.name.clone()
            }
        })
        .unwrap_or_else(|| provider.to_string())
}

fn model_display_label(settings: &AppUiSettings, active: &ActiveModel) -> String {
    for section in native_picker_sections(settings) {
        if section.slug != active.provider {
            continue;
        }
        if let Some(entry) = section.models.iter().find(|model| model.id == active.model) {
            return entry.display_label().to_string();
        }
    }
    active.model.clone()
}

fn native_picker_sections(settings: &AppUiSettings) -> Vec<NativePickerSection> {
    let mut sections = Vec::new();
    for spec in builtin_providers() {
        if spec.kind != AiProviderKind::NativeHttp {
            continue;
        }
        let builtin: Vec<AiModelEntry> = spec
            .builtin_models
            .iter()
            .map(|(id, label)| AiModelEntry {
                id: (*id).to_string(),
                label: (*label).to_string(),
            })
            .collect();
        let extra = settings
            .ai_catalog
            .overrides
            .get(spec.slug)
            .map(|over| over.extra_models.as_slice())
            .unwrap_or(&[]);
        let hidden = settings
            .ai_catalog
            .overrides
            .get(spec.slug)
            .map(|over| over.hidden_builtin_ids.as_slice())
            .unwrap_or(&[]);
        sections.push(NativePickerSection {
            slug: spec.slug.to_string(),
            label: spec.label.to_string(),
            supports_refresh: spec.supports_model_refresh,
            models: resolve_picker_models(&builtin, extra, hidden),
        });
    }
    for custom in &settings.ai_catalog.custom_native {
        let label = if custom.name.trim().is_empty() {
            custom.id.clone()
        } else {
            custom.name.clone()
        };
        sections.push(NativePickerSection {
            slug: custom.id.clone(),
            label,
            supports_refresh: true,
            models: custom.models.clone(),
        });
    }
    sections
}

fn acp_picker_agents() -> Vec<(String, String)> {
    builtin_providers()
        .iter()
        .filter(|spec| spec.kind == AiProviderKind::Acp)
        .map(|spec| (spec.slug.to_string(), spec.label.to_string()))
        .collect()
}

fn native_refresh_base_url(settings: &AppUiSettings, provider: &str) -> String {
    if let Some(custom) = settings
        .ai_catalog
        .custom_native
        .iter()
        .find(|custom| custom.id == provider)
    {
        return custom.base_url.clone();
    }
    let default = builtin_providers()
        .iter()
        .find(|spec| spec.slug == provider)
        .map(|spec| spec.default_base_url)
        .unwrap_or("");
    if let Some(over) = settings.ai_catalog.overrides.get(provider)
        && !over.base_url.trim().is_empty()
    {
        return over.base_url.clone();
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
    if vendor.trim().is_empty() {
        default.to_string()
    } else {
        vendor.to_string()
    }
}

fn merge_refreshed_models(slug: &str, fetched: Vec<AiModelEntry>) {
    crate::app_state::update_ui_settings(|current| {
        if let Some(custom) = current
            .ai_catalog
            .custom_native
            .iter_mut()
            .find(|custom| custom.id == slug)
        {
            for model in fetched {
                if custom.models.iter().any(|existing| existing.id == model.id) {
                    continue;
                }
                custom.models.push(model);
            }
            return;
        }
        let builtin_ids: Vec<&str> = builtin_providers()
            .iter()
            .find(|spec| spec.slug == slug)
            .map(|spec| spec.builtin_models.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default();
        let extra = &mut current
            .ai_catalog
            .overrides
            .entry(slug.to_string())
            .or_default()
            .extra_models;
        for model in fetched {
            if builtin_ids.iter().any(|id| *id == model.id) {
                continue;
            }
            if extra.iter().any(|existing| existing.id == model.id) {
                continue;
            }
            extra.push(model);
        }
    });
}

fn refresh_picker_models(slug: String) {
    let settings = crate::app_state::APP_UI_SETTINGS();
    let base_url = native_refresh_base_url(&settings, &slug);
    let api_key = settings.lm_api_key(&slug);
    spawn(async move {
        match services::refresh_provider_models(&slug, &base_url, &api_key).await {
            Ok(models) => merge_refreshed_models(&slug, models),
            Err(err) => crate::app_state::toast_error(err),
        }
    });
}

fn connect_acp_from_picker(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    provider: String,
) {
    spawn(async move {
        let result = match provider.as_str() {
            "acp:opencode" => ensure_opencode_connected(panel_state, chat_revision).await,
            "acp:codex" =>
                connect_registry_agent(panel_state, chat_revision, "codex-acp", "Codex CLI").await,
            _ => Ok(()),
        };
        if result.is_ok() {
            crate::app_state::set_active_model(provider, String::new());
        }
    });
}
