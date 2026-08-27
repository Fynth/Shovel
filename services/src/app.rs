use std::collections::BTreeMap;

use futures_util::future::join_all;
use models::{
    AppBehavior,
    AppUiSettings,
    ConnectionRequest,
    DatabaseConnection,
    EditorBehavior,
    KeybindingMap,
    PanelBehavior,
    ShovelConfig,
    SqlFormatSettings,
    ThemeOverrides,
};

/// Legacy keyring service names for builtin LM providers (pre-`shovel.lm.<id>`).
const LEGACY_LM_KEYRING: &[(&str, &str)] = &[
    ("deepseek", "shovel.deepseek"),
    ("openai", "shovel.openai"),
    ("groq", "shovel.groq"),
    ("openrouter", "shovel.openrouter"),
    ("xai", "shovel.xai"),
    ("mistral", "shovel.mistral"),
    ("ollama", "shovel.ollama"),
];

#[derive(Clone, Debug, Default)]
pub struct AppStartupSettings {
    pub ui_settings: AppUiSettings,
    pub sql_format_settings: SqlFormatSettings,
    /// Connections declared in `config.toml` with `auto_connect = true`.
    pub config_connections: Vec<models::ConfigConnection>,
    /// Deep-customization overrides from `config.toml`.
    pub theme_overrides: Option<ThemeOverrides>,
    pub keybindings: Option<KeybindingMap>,
    pub editor: Option<EditorBehavior>,
    pub panels: Option<PanelBehavior>,
    pub behavior: Option<AppBehavior>,
}

#[derive(Clone, Debug)]
pub struct ConnectAndSaveResult {
    pub connection: DatabaseConnection,
    pub save_warning: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SessionRestoreResult {
    pub restored: Vec<(ConnectionRequest, DatabaseConnection)>,
    pub active_connection_name: Option<String>,
    pub failed_requests: Vec<(ConnectionRequest, String)>,
    pub tab_drafts: Vec<models::TabDraft>,
}

pub async fn load_app_startup_settings() -> Result<AppStartupSettings, String> {
    let mut ui_settings = storage::load_app_ui_settings().await?;
    let sql_format_settings = storage::load_sql_format_settings().await?;

    // Apply the optional `config.toml` over the persisted settings so a
    // user can customize the app declaratively without touching the UI.
    let mut config_connections = Vec::new();
    let mut theme_overrides = None;
    let mut keybindings = None;
    let mut editor = None;
    let mut panels = None;
    let mut behavior = None;
    if let Some(config) = load_shovel_config()? {
        config.apply_to(&mut ui_settings);
        config_connections = config
            .connections
            .into_iter()
            .filter(|connection| connection.auto_connect)
            .collect();
        theme_overrides = config.theme_overrides;
        keybindings = config.keybindings;
        editor = config.editor;
        panels = config.panels;
        behavior = config.behavior;
    }

    hydrate_secret(
        &mut ui_settings.codestral.api_key,
        storage::load_codestral_api_key().await?,
        storage::save_codestral_api_key,
    )
    .await?;

    hydrate_lm_keys(&mut ui_settings).await?;

    Ok(AppStartupSettings {
        ui_settings,
        sql_format_settings,
        config_connections,
        theme_overrides,
        keybindings,
        editor,
        panels,
        behavior,
    })
}

/// Locate and parse the user's `config.toml`. Looks in the app data dir
/// first, then the current working directory, so a repo-local config is
/// honored when running from a checkout. On first launch (no config found
/// anywhere) a default template is written to the app data dir so the user
/// has a discoverable, editable file.
fn load_shovel_config() -> Result<Option<ShovelConfig>, String> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join("shovel");
    let data_config = data_dir.join("config.toml");
    let cwd_config = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("config.toml");

    for path in [&data_config, &cwd_config] {
        if let Some(config) = ShovelConfig::load(path)? {
            return Ok(Some(config));
        }
    }

    // No config anywhere — write a default template so the user can edit it.
    let template = ShovelConfig::default();
    if let Err(err) = template.save(&data_config) {
        eprintln!("Failed to write default config.toml: {err}");
    }
    Ok(None)
}

pub async fn save_app_ui_settings_with_secrets(settings: AppUiSettings) -> Result<(), String> {
    let codestral_api_key = settings.codestral.api_key.clone();
    let lm_keys = collect_lm_keys_for_save(&settings);

    storage::save_app_ui_settings(settings)
        .await
        .map_err(|err| {
            format!("failed to save UI settings metadata before storing secure secrets: {err}")
        })?;

    let mut secret_errors = Vec::new();
    if let Err(err) = storage::save_codestral_api_key(codestral_api_key).await {
        secret_errors.push(format!("CodeStral: {err}"));
    }
    for (provider_id, api_key) in lm_keys {
        let service = storage::lm_service_name(&provider_id);
        if let Err(err) = storage::save_lm_api_key(&service, api_key).await {
            secret_errors.push(format!("{provider_id}: {err}"));
        }
    }

    if secret_errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "saved UI settings metadata, but secure storage had issues: {}",
            secret_errors.join("; ")
        ))
    }
}

/// Copy legacy `shovel.<slug>` keys into `shovel.lm.<slug>` when the new
/// service is empty, then hydrate `lm_keys` (and legacy vendor `api_key`
/// fields for one-release UI compat).
async fn hydrate_lm_keys(settings: &mut AppUiSettings) -> Result<(), String> {
    for &(slug, legacy_service) in LEGACY_LM_KEYRING {
        let new_service = storage::lm_service_name(slug);
        let mut key = storage::load_lm_api_key(&new_service).await?;
        if key.trim().is_empty() {
            let legacy = storage::load_lm_api_key(legacy_service).await?;
            if !legacy.trim().is_empty() {
                // Best-effort copy; fallback inside save_lm_api_key handles
                // dead keyrings. Do not fail startup if the copy warns.
                let _ = storage::save_lm_api_key(&new_service, legacy.clone()).await;
                key = legacy;
            }
        }
        if key.trim().is_empty() {
            // Plaintext leftover from older JSON (skip_serializing only).
            key = legacy_vendor_api_key(settings, slug).to_string();
            if !key.trim().is_empty() {
                let _ = storage::save_lm_api_key(&new_service, key.clone()).await;
            }
        }
        if !key.trim().is_empty() {
            settings.lm_keys.insert(slug.to_string(), key.clone());
            set_legacy_vendor_api_key(settings, slug, key);
        }
    }

    for custom in &settings.ai_catalog.custom_native {
        let service = storage::lm_service_name(&custom.id);
        let key = storage::load_lm_api_key(&service).await?;
        if !key.trim().is_empty() {
            settings.lm_keys.insert(custom.id.clone(), key);
        }
    }

    Ok(())
}

fn legacy_vendor_api_key<'a>(settings: &'a AppUiSettings, slug: &str) -> &'a str {
    match slug {
        "deepseek" => settings.deepseek.api_key.as_str(),
        "openai" => settings.openai.api_key.as_str(),
        "groq" => settings.groq.api_key.as_str(),
        "openrouter" => settings.openrouter.api_key.as_str(),
        "xai" => settings.xai.api_key.as_str(),
        "mistral" => settings.mistral.api_key.as_str(),
        "ollama" => settings.ollama.api_key.as_str(),
        _ => "",
    }
}

fn set_legacy_vendor_api_key(settings: &mut AppUiSettings, slug: &str, key: String) {
    match slug {
        "deepseek" => settings.deepseek.api_key = key,
        "openai" => settings.openai.api_key = key,
        "groq" => settings.groq.api_key = key,
        "openrouter" => settings.openrouter.api_key = key,
        "xai" => settings.xai.api_key = key,
        "mistral" => settings.mistral.api_key = key,
        "ollama" => settings.ollama.api_key = key,
        _ => {}
    }
}

/// Merge in-memory `lm_keys` with legacy vendor `api_key` fields so the
/// settings modal (still editing per-vendor blobs) keeps working until Task 6.
fn collect_lm_keys_for_save(settings: &AppUiSettings) -> BTreeMap<String, String> {
    let mut keys = settings.lm_keys.clone();
    for &(slug, _) in LEGACY_LM_KEYRING {
        let legacy = legacy_vendor_api_key(settings, slug).to_string();
        // Overlay when the UI still owns the vendor blob, including empty
        // (clear). Skip untouched providers so a theme-only save does not
        // delete unrelated keyring entries.
        if !legacy.trim().is_empty() || keys.contains_key(slug) {
            keys.insert(slug.to_string(), legacy);
        }
    }
    keys
}

async fn hydrate_secret<Fut>(
    target: &mut String,
    secure_value: String,
    save_legacy: impl Fn(String) -> Fut,
) -> Result<(), String>
where
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if secure_value.trim().is_empty() {
        let legacy_value = target.trim().to_string();
        if !legacy_value.is_empty() {
            save_legacy(legacy_value.clone()).await?;
            *target = legacy_value;
        }
    } else {
        *target = secure_value;
    }

    Ok(())
}

pub async fn restore_saved_sessions() -> Result<SessionRestoreResult, String> {
    let (open_requests, active_connection_name, tab_drafts) = storage::load_session_state().await?;
    if open_requests.is_empty() {
        return Ok(SessionRestoreResult {
            active_connection_name,
            tab_drafts,
            ..SessionRestoreResult::default()
        });
    }

    let restored_results = join_all(open_requests.into_iter().map(|request| async move {
        match connection::connect_to_db(request.clone()).await {
            Ok(connection) => Ok((request, connection)),
            Err(err) => Err((request, err.to_string())),
        }
    }))
    .await;

    let mut restored = Vec::new();
    let mut failed_requests = Vec::new();
    for result in restored_results {
        match result {
            Ok(value) => restored.push(value),
            Err(value) => failed_requests.push(value),
        }
    }

    Ok(SessionRestoreResult {
        restored,
        active_connection_name,
        failed_requests,
        tab_drafts,
    })
}

pub async fn connect_and_save_request(
    request: ConnectionRequest,
) -> Result<ConnectAndSaveResult, String> {
    let connection = connection::connect_to_db(request.clone())
        .await
        .map_err(|err| err.to_string())?;
    let save_warning = storage::save_connection_request(request).await.err();

    Ok(ConnectAndSaveResult {
        connection,
        save_warning,
    })
}
