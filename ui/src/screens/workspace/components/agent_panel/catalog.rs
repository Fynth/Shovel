use dioxus::prelude::*;
use models::{
    AcpPanelState,
    ActiveModel,
    AiModelEntry,
    AiProviderGroup,
    AiProviderKind,
    AppUiSettings,
    builtin_providers,
    is_native_http_ready,
    native_http_provider_enabled,
    needs_acp_reconnect,
    provider_backend,
    provider_kind,
    resolve_picker_models,
};

use super::{
    requests::native_base_url,
    setup::{apply_native_connected_signal, connect_registry_agent, native_provider_label},
};

#[component]
pub(super) fn AgentModelPicker(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    busy: bool,
) -> Element {
    let mut show_picker = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut selected_slug = use_signal(String::new);
    let settings = crate::app_state::APP_UI_SETTINGS();
    let trigger_label = picker_trigger_label(&settings);
    let native_sections = native_picker_sections(&settings);
    let acp_agents = acp_picker_agents();
    let active = settings.ai_catalog.active.clone();
    let picker_locked = busy || panel_state().busy;
    let trigger_class = if show_picker() {
        "button button--ghost button--small button--active agent-panel__model-trigger"
    } else {
        "button button--ghost button--small agent-panel__model-trigger"
    };

    let q = query().trim().to_ascii_lowercase();
    let visible_sections: Vec<NativePickerSection> = native_sections
        .into_iter()
        .map(|mut section| {
            if !q.is_empty() {
                section.models.retain(|model| model_matches(model, &q));
            }
            section
        })
        .filter(|section| {
            if q.is_empty() {
                return true;
            }
            !section.models.is_empty()
                || section.label.to_ascii_lowercase().contains(&q)
                || section.slug.to_ascii_lowercase().contains(&q)
        })
        .collect();
    let selected_slug_value = {
        let current = selected_slug();
        if visible_sections
            .iter()
            .any(|section| section.slug == current)
        {
            current
        } else {
            active
                .as_ref()
                .map(|current| current.provider.clone())
                .filter(|slug| visible_sections.iter().any(|section| section.slug == *slug))
                .or_else(|| visible_sections.first().map(|section| section.slug.clone()))
                .unwrap_or_default()
        }
    };
    let selected_section = visible_sections
        .iter()
        .find(|section| section.slug == selected_slug_value)
        .cloned();

    rsx! {
        div { class: "agent-panel__composer-footer",
            button {
                class: trigger_class,
                disabled: picker_locked,
                title: "Switch language model",
                "aria-expanded": "{show_picker()}",
                onclick: move |_| {
                    if panel_state().busy {
                        return;
                    }
                    show_picker.set(!show_picker());
                },
                {trigger_label}
            }
            if show_picker() {
                div {
                    class: "agent-panel__model-picker",
                    onclick: move |event| event.stop_propagation(),
                    input {
                        class: "input agent-panel__model-picker-search",
                        r#type: "search",
                        placeholder: "Search models or providers",
                        value: "{query}",
                        oninput: move |event| query.set(event.value()),
                    }
                    if visible_sections.is_empty() && acp_agents.is_empty() {
                        p { class: "agent-panel__hint",
                            "Enable a provider in the Providers panel, then switch models here."
                        }
                    }
                    if !visible_sections.is_empty() {
                        div { class: "agent-panel__model-picker-providers",
                            for section in visible_sections.iter() {
                                {
                                    let slug = section.slug.clone();
                                    let is_selected = selected_section
                                        .as_ref()
                                        .is_some_and(|current| current.slug == slug);
                                    let chip_class = if is_selected {
                                        "button button--ghost button--small button--active"
                                    } else {
                                        "button button--ghost button--small"
                                    };
                                    rsx! {
                                        button {
                                            class: chip_class,
                                            disabled: picker_locked,
                                            onclick: move |event| {
                                                event.stop_propagation();
                                                selected_slug.set(slug.clone());
                                            },
                                            {section.label.clone()}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(section) = selected_section {
                        {
                            let refresh_slug = section.slug.clone();
                            let section_slug = section.slug.clone();
                            rsx! {
                                div { class: "agent-panel__model-picker-section",
                                    div { class: "agent-panel__model-picker-header",
                                        span { {section.label} }
                                        if section.supports_refresh {
                                            button {
                                                class: "button button--ghost button--small",
                                                disabled: picker_locked,
                                                onclick: move |event| {
                                                    event.stop_propagation();
                                                    if panel_state().busy {
                                                        return;
                                                    }
                                                    refresh_picker_models(refresh_slug.clone());
                                                },
                                                "Refresh"
                                            }
                                        }
                                    }
                                    if section.models.is_empty() {
                                        p { class: "agent-panel__hint", "No models match." }
                                    }
                                    for model in section.models {
                                        {
                                            let provider = section_slug.clone();
                                            let model_id = model.id.clone();
                                            let previous = active.clone();
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
                                                    disabled: picker_locked,
                                                    onclick: move |_| {
                                                        if panel_state().busy {
                                                            return;
                                                        }
                                                        apply_active_model_change(
                                                            panel_state,
                                                            chat_revision,
                                                            previous.clone(),
                                                            ActiveModel {
                                                                provider: provider.clone(),
                                                                model: model_id.clone(),
                                                            },
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
                            let previous = active.clone();
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
                                    disabled: picker_locked,
                                    onclick: move |_| {
                                        if panel_state().busy {
                                            return;
                                        }
                                        apply_active_model_change(
                                            panel_state,
                                            chat_revision,
                                            previous.clone(),
                                            ActiveModel {
                                                provider: slug.clone(),
                                                model: String::new(),
                                            },
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

#[component]
pub(super) fn AgentProvidersPopover(
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    busy: bool,
) -> Element {
    let mut query = use_signal(String::new);
    let settings = crate::app_state::APP_UI_SETTINGS();
    let q = query().trim().to_ascii_lowercase();
    let rows = provider_switch_rows(&settings)
        .into_iter()
        .filter(|row| {
            if q.is_empty() {
                return true;
            }
            row.label.to_ascii_lowercase().contains(&q)
                || row.slug.to_ascii_lowercase().contains(&q)
                || row.models.iter().any(|model| model_matches(model, &q))
        })
        .collect::<Vec<_>>();
    let groups = [
        AiProviderGroup::Subscription,
        AiProviderGroup::Cloud,
        AiProviderGroup::Local,
        AiProviderGroup::Agent,
    ];

    rsx! {
        div {
            class: "agent-panel__providers-popover",
            onclick: move |event| event.stop_propagation(),
            div { class: "agent-panel__dialogs-header",
                div { class: "agent-panel__dialogs-copy",
                    h4 { class: "agent-panel__section-title", "Providers" }
                    p { class: "agent-panel__hint",
                        "Enable a key, then Use — native models hot-switch without reconnecting."
                    }
                }
            }
            input {
                class: "input agent-panel__model-picker-search",
                r#type: "search",
                placeholder: "Search providers",
                value: "{query}",
                oninput: move |event| query.set(event.value()),
            }
            div { class: "agent-panel__providers-list",
                for group in groups {
                    {
                        let group_rows: Vec<ProviderSwitchRowData> = rows
                            .iter()
                            .filter(|row| row.group == group)
                            .cloned()
                            .collect();
                        rsx! {
                            if !group_rows.is_empty() {
                                p { class: "agent-panel__model-picker-group-title", {group.label()} }
                                for row in group_rows {
                                    ProviderSwitchRow {
                                        key: "{row.slug}",
                                        row,
                                        panel_state,
                                        chat_revision,
                                        busy,
                                    }
                                }
                            }
                        }
                    }
                }
                if rows.is_empty() {
                    p { class: "empty-state", "No providers match." }
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

#[derive(Clone, PartialEq)]
struct ProviderSwitchRowData {
    slug: String,
    label: String,
    group: AiProviderGroup,
    kind: AiProviderKind,
    enabled: bool,
    api_key: String,
    supports_refresh: bool,
    models: Vec<AiModelEntry>,
    active_model: String,
}

#[component]
fn ProviderSwitchRow(
    row: ProviderSwitchRowData,
    panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    busy: bool,
) -> Element {
    let settings = crate::app_state::APP_UI_SETTINGS();
    let active = settings.ai_catalog.active.clone();
    let is_active = active
        .as_ref()
        .is_some_and(|current| current.provider == row.slug);
    let picker_locked = busy || panel_state().busy;
    let mut model_draft = use_signal(|| row.active_model.clone());
    let selected_model = {
        let draft = model_draft();
        if !draft.trim().is_empty() {
            draft
        } else {
            row.models
                .first()
                .map(|model| model.id.clone())
                .unwrap_or_default()
        }
    };
    let slug = row.slug.clone();
    let enable_slug = row.slug.clone();
    let key_slug = row.slug.clone();
    let refresh_slug = row.slug.clone();
    let use_slug = row.slug.clone();
    let use_previous = active.clone();
    let models = row.models.clone();

    rsx! {
        article {
            class: if is_active {
                "agent-panel__provider-row agent-panel__provider-row--active"
            } else {
                "agent-panel__provider-row"
            },
            div { class: "agent-panel__provider-row-header",
                if row.kind == AiProviderKind::NativeHttp {
                    label { class: "settings-modal__toggle",
                        input {
                            r#type: "checkbox",
                            checked: row.enabled,
                            oninput: move |event| {
                                let checked = event.checked();
                                let slug = enable_slug.clone();
                                crate::app_state::update_ui_settings(move |current| {
                                    current
                                        .ai_catalog
                                        .overrides
                                        .entry(slug)
                                        .or_default()
                                        .enabled = checked;
                                });
                            },
                        }
                        span { {row.label.clone()} }
                    }
                } else {
                    span { class: "agent-panel__provider-row-title", {row.label.clone()} }
                }
                if row.kind == AiProviderKind::NativeHttp && row.supports_refresh {
                    button {
                        class: "button button--ghost button--small",
                        disabled: picker_locked,
                        onclick: move |_| refresh_picker_models(refresh_slug.clone()),
                        "Refresh"
                    }
                }
            }
            if row.kind == AiProviderKind::NativeHttp {
                div { class: "agent-panel__provider-row-fields",
                    input {
                        class: "input",
                        r#type: "password",
                        placeholder: "API key",
                        value: "{row.api_key}",
                        oninput: move |event| {
                            let value = event.value();
                            let slug = key_slug.clone();
                            crate::app_state::update_ui_settings(move |current| {
                                current.set_lm_api_key(&slug, value);
                            });
                        },
                    }
                    select {
                        class: "input",
                        value: selected_model.clone(),
                        onchange: move |event| model_draft.set(event.value()),
                        for model in models {
                            option {
                                value: "{model.id}",
                                {model.display_label().to_string()}
                            }
                        }
                    }
                    button {
                        class: "button button--primary button--small",
                        disabled: picker_locked || selected_model.trim().is_empty(),
                        onclick: move |_| {
                            if panel_state().busy {
                                return;
                            }
                            let slug = use_slug.clone();
                            crate::app_state::update_ui_settings({
                                let slug = slug.clone();
                                move |current| {
                                    current
                                        .ai_catalog
                                        .overrides
                                        .entry(slug)
                                        .or_default()
                                        .enabled = true;
                                }
                            });
                            apply_active_model_change(
                                panel_state,
                                chat_revision,
                                use_previous.clone(),
                                ActiveModel {
                                    provider: slug,
                                    model: model_draft(),
                                },
                            );
                        },
                        {if is_active { "Using" } else { "Use" }}
                    }
                }
            } else {
                button {
                    class: "button button--primary button--small",
                    disabled: picker_locked,
                    onclick: move |_| {
                        if panel_state().busy {
                            return;
                        }
                        apply_active_model_change(
                            panel_state,
                            chat_revision,
                            use_previous.clone(),
                            ActiveModel {
                                provider: slug.clone(),
                                model: String::new(),
                            },
                        );
                    },
                    {if is_active { "Connected" } else { "Connect" }}
                }
            }
        }
    }
}

fn model_matches(model: &AiModelEntry, q: &str) -> bool {
    model.id.to_ascii_lowercase().contains(q) || model.label.to_ascii_lowercase().contains(q)
}

fn picker_trigger_label(settings: &AppUiSettings) -> String {
    let Some(active) = settings.ai_catalog.active.as_ref() else {
        return "Select model".to_string();
    };
    let label = native_provider_label(settings, &active.provider);
    let model = model_display_label(settings, active);
    if model.trim().is_empty() {
        label
    } else {
        format!("{label} / {model}")
    }
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
        if spec.kind() != AiProviderKind::NativeHttp {
            continue;
        }
        if !native_http_provider_enabled(&settings.ai_catalog, spec.slug) {
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
            supports_refresh: spec.supports_model_refresh(),
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

fn provider_switch_rows(settings: &AppUiSettings) -> Vec<ProviderSwitchRowData> {
    let active = settings.ai_catalog.active.as_ref();
    let mut rows = Vec::new();
    for spec in builtin_providers() {
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
        let builtin: Vec<AiModelEntry> = spec
            .builtin_models
            .iter()
            .map(|(id, label)| AiModelEntry {
                id: (*id).to_string(),
                label: (*label).to_string(),
            })
            .collect();
        let models = if spec.kind() == AiProviderKind::NativeHttp {
            resolve_picker_models(&builtin, extra, hidden)
        } else {
            Vec::new()
        };
        let active_model = active
            .filter(|current| current.provider == spec.slug)
            .map(|current| current.model.clone())
            .filter(|model| !model.trim().is_empty())
            .or_else(|| models.first().map(|model| model.id.clone()))
            .unwrap_or_default();
        rows.push(ProviderSwitchRowData {
            slug: spec.slug.to_string(),
            label: spec.label.to_string(),
            group: spec.group,
            kind: spec.kind(),
            enabled: native_http_provider_enabled(&settings.ai_catalog, spec.slug),
            api_key: settings.lm_api_key(spec.slug),
            supports_refresh: spec.supports_model_refresh(),
            models,
            active_model,
        });
    }
    for custom in &settings.ai_catalog.custom_native {
        let label = if custom.name.trim().is_empty() {
            custom.id.clone()
        } else {
            custom.name.clone()
        };
        let active_model = active
            .filter(|current| current.provider == custom.id)
            .map(|current| current.model.clone())
            .filter(|model| !model.trim().is_empty())
            .or_else(|| custom.models.first().map(|model| model.id.clone()))
            .unwrap_or_default();
        rows.push(ProviderSwitchRowData {
            slug: custom.id.clone(),
            label,
            group: AiProviderGroup::Cloud,
            kind: AiProviderKind::NativeHttp,
            enabled: true,
            api_key: settings.lm_api_key(&custom.id),
            supports_refresh: true,
            models: custom.models.clone(),
            active_model,
        });
    }
    rows
}

fn acp_picker_agents() -> Vec<(String, String)> {
    builtin_providers()
        .iter()
        .filter(|spec| spec.kind() == AiProviderKind::Acp)
        .map(|spec| (spec.slug.to_string(), spec.label.to_string()))
        .collect()
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
    let base_url = native_base_url(&settings, &slug);
    let api_key = settings.lm_api_key(&slug);
    let Some(backend) = provider_backend(&slug, &settings.ai_catalog) else {
        crate::app_state::toast_error("This provider cannot refresh models.");
        return;
    };
    spawn(async move {
        match services::refresh_provider_models(backend, &base_url, &api_key).await {
            Ok(models) => merge_refreshed_models(&slug, models),
            Err(err) => crate::app_state::toast_error(err),
        }
    });
}

pub(super) fn apply_active_model_change(
    mut panel_state: Signal<AcpPanelState>,
    chat_revision: Signal<u64>,
    previous: Option<ActiveModel>,
    next: ActiveModel,
) {
    let reconnect = match previous.as_ref() {
        Some(prev) => needs_acp_reconnect(&prev.provider, &next.provider),
        None => provider_kind(&next.provider) == Some(AiProviderKind::Acp),
    };

    crate::app_state::set_active_model(next.provider.clone(), next.model.clone());

    if !reconnect {
        if provider_kind(&next.provider) == Some(AiProviderKind::NativeHttp) {
            apply_native_http_session(panel_state, &next.provider);
        }
        return;
    }

    panel_state.with_mut(|state| {
        state.busy = true;
        state.status = "Switching language model...".to_string();
    });

    spawn(async move {
        super::disconnect_acp_runtime_if_needed(&panel_state());

        let outcome = match provider_kind(&next.provider) {
            Some(AiProviderKind::Acp) =>
                connect_acp_provider(panel_state, chat_revision, &next.provider).await,
            Some(AiProviderKind::NativeHttp) => {
                apply_native_http_session(panel_state, &next.provider);
                Ok(())
            }
            None => {
                panel_state.with_mut(|state| {
                    state.busy = false;
                });
                Ok(())
            }
        };

        if let Err(err) = outcome {
            restore_active_model(previous.as_ref());
            if let Some(prev) = previous.as_ref()
                && provider_kind(&prev.provider) == Some(AiProviderKind::NativeHttp)
            {
                apply_native_http_session(panel_state, &prev.provider);
            } else {
                panel_state.with_mut(|state| {
                    state.busy = false;
                });
            }
            crate::app_state::toast_error(err);
        }
    });
}

fn restore_active_model(previous: Option<&ActiveModel>) {
    crate::app_state::update_ui_settings(|current| {
        current.ai_catalog.active = previous.cloned();
    });
}

fn apply_native_http_session(mut panel_state: Signal<AcpPanelState>, provider: &str) {
    let settings = crate::app_state::APP_UI_SETTINGS();
    let key = settings.lm_api_key(provider);
    let label = native_provider_label(&settings, provider);
    apply_native_connected_signal(panel_state, label.clone());
    if !native_http_provider_enabled(&settings.ai_catalog, provider) {
        panel_state.with_mut(|state| {
            state.status = format!("{label} is disabled.");
        });
    } else if !is_native_http_ready(provider, &key, &settings.ai_catalog) {
        panel_state.with_mut(|state| {
            state.status = format!("Add an API key for {label}.");
        });
    }
}

async fn connect_acp_provider(
    mut panel_state: Signal<AcpPanelState>,
    mut chat_revision: Signal<u64>,
    provider: &str,
) -> Result<(), String> {
    match provider {
        "acp:opencode" =>
            connect_registry_agent(panel_state, chat_revision, "opencode", "OpenCode").await,
        "acp:codex" =>
            connect_registry_agent(panel_state, chat_revision, "codex-acp", "Codex CLI").await,
        _ => {
            let launch = panel_state().launch.clone();
            match services::connect_acp_agent(launch).await {
                Ok(connection) => {
                    panel_state.with_mut(|state| {
                        super::state::apply_connected(state, connection);
                    });
                    Ok(())
                }
                Err(err) => {
                    panel_state.with_mut(|state| {
                        state.busy = false;
                        state.connected = false;
                        state.connection = None;
                        state.status = err.clone();
                    });
                    chat_revision += 1;
                    Err(err)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{native_picker_sections, picker_trigger_label};
    use models::{ActiveModel, AiProviderOverride, AppUiSettings};

    #[test]
    fn native_picker_skips_disabled_builtins() {
        let mut settings = AppUiSettings::default();
        settings.ai_catalog.overrides.insert(
            "openai".into(),
            AiProviderOverride {
                enabled: true,
                ..Default::default()
            },
        );
        let slugs: Vec<_> = native_picker_sections(&settings)
            .into_iter()
            .map(|section| section.slug)
            .collect();
        assert!(slugs.iter().any(|slug| slug == "openai"));
        assert!(!slugs.iter().any(|slug| slug == "deepseek"));
    }

    #[test]
    fn picker_trigger_label_omits_slash_when_model_empty() {
        let mut settings = AppUiSettings::default();
        settings.ai_catalog.active = Some(ActiveModel {
            provider: "acp:opencode".into(),
            model: String::new(),
        });
        assert_eq!(picker_trigger_label(&settings), "OpenCode");
        settings.ai_catalog.active = Some(ActiveModel {
            provider: "acp:codex".into(),
            model: "  ".into(),
        });
        assert_eq!(picker_trigger_label(&settings), "Codex");
    }

    #[test]
    fn picker_trigger_label_joins_provider_and_model() {
        let mut settings = AppUiSettings::default();
        settings.ai_catalog.active = Some(ActiveModel {
            provider: "openai".into(),
            model: "gpt-4o".into(),
        });
        assert_eq!(picker_trigger_label(&settings), "OpenAI / gpt-4o");
    }
}
