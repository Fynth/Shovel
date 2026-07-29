use futures_util::future::join_all;
use models::{AppUiSettings, ConnectionRequest, DatabaseConnection, SqlFormatSettings};

#[derive(Clone, Debug)]
pub struct AppStartupSettings {
    pub ui_settings: AppUiSettings,
    pub sql_format_settings: SqlFormatSettings,
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

    hydrate_secret(
        &mut ui_settings.codestral.api_key,
        storage::load_codestral_api_key().await?,
        storage::save_codestral_api_key,
    )
    .await?;
    hydrate_secret(
        &mut ui_settings.deepseek.api_key,
        storage::load_deepseek_api_key().await?,
        storage::save_deepseek_api_key,
    )
    .await?;

    Ok(AppStartupSettings {
        ui_settings,
        sql_format_settings,
    })
}

pub async fn save_app_ui_settings_with_secrets(settings: AppUiSettings) -> Result<(), String> {
    let codestral_api_key = settings.codestral.api_key.clone();
    let deepseek_api_key = settings.deepseek.api_key.clone();

    storage::save_app_ui_settings(settings)
        .await
        .map_err(|err| {
            format!("failed to save UI settings metadata before storing secure secrets: {err}")
        })?;

    let mut secret_errors = Vec::new();
    if let Err(err) = storage::save_codestral_api_key(codestral_api_key).await {
        secret_errors.push(format!("CodeStral: {err}"));
    }
    if let Err(err) = storage::save_deepseek_api_key(deepseek_api_key).await {
        secret_errors.push(format!("DeepSeek: {err}"));
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
