use keyring::{Entry, Error as KeyringError};
use models::{AppUiSettings, SqlFormatSettings};

use crate::{
    fs_store::{app_ui_settings_path, read_json_file, sql_format_settings_path, write_json_file},
    secrets::{delete_fallback_secret, load_fallback_secret, save_fallback_secret},
};

const CODESTRAL_KEYRING_SERVICE: &str = "shovel.codestral";
const CODESTRAL_KEYRING_ACCOUNT: &str = "default";
const DEEPSEEK_KEYRING_SERVICE: &str = "shovel.deepseek";
const DEEPSEEK_KEYRING_ACCOUNT: &str = "default";

pub async fn load_app_ui_settings() -> Result<AppUiSettings, String> {
    read_json_file(app_ui_settings_path()).await
}

pub async fn save_app_ui_settings(settings: AppUiSettings) -> Result<(), String> {
    write_json_file(app_ui_settings_path(), &settings).await
}

pub async fn load_sql_format_settings() -> Result<SqlFormatSettings, String> {
    read_json_file(sql_format_settings_path()).await
}

pub async fn save_sql_format_settings(settings: SqlFormatSettings) -> Result<(), String> {
    write_json_file(sql_format_settings_path(), &settings).await
}

pub async fn load_codestral_api_key() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        load_api_key_sync(CODESTRAL_KEYRING_SERVICE, CODESTRAL_KEYRING_ACCOUNT)
    })
    .await
    .map_err(|err| format!("failed to join CodeStral secret task: {err}"))?
}

pub async fn save_codestral_api_key(api_key: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        save_api_key_sync(
            CODESTRAL_KEYRING_SERVICE,
            CODESTRAL_KEYRING_ACCOUNT,
            &api_key,
        )
    })
    .await
    .map_err(|err| format!("failed to join CodeStral secret task: {err}"))?
}

pub async fn load_deepseek_api_key() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        load_api_key_sync(DEEPSEEK_KEYRING_SERVICE, DEEPSEEK_KEYRING_ACCOUNT)
    })
    .await
    .map_err(|err| format!("failed to join DeepSeek secret task: {err}"))?
}

pub async fn save_deepseek_api_key(api_key: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        save_api_key_sync(DEEPSEEK_KEYRING_SERVICE, DEEPSEEK_KEYRING_ACCOUNT, &api_key)
    })
    .await
    .map_err(|err| format!("failed to join DeepSeek secret task: {err}"))?
}

fn load_api_key_sync(service: &str, account: &str) -> Result<String, String> {
    let entry = Entry::new(service, account);
    match entry {
        Ok(entry) => match entry.get_password() {
            Ok(api_key) => {
                let _ = delete_fallback_secret(service, account);
                Ok(api_key)
            }
            Err(KeyringError::NoEntry) =>
                Ok(load_fallback_secret(service, account)?.unwrap_or_default()),
            Err(_) => Ok(load_fallback_secret(service, account)?.unwrap_or_default()),
        },
        Err(_) => Ok(load_fallback_secret(service, account)?.unwrap_or_default()),
    }
}

fn save_api_key_sync(service: &str, account: &str, api_key: &str) -> Result<(), String> {
    let entry = Entry::new(service, account);

    if api_key.trim().is_empty() {
        if let Ok(entry) = entry {
            match entry.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => {}
                Err(_) => {}
            }
        }
        delete_fallback_secret(service, account)
    } else if let Ok(entry) = entry {
        match entry.set_password(api_key) {
            Ok(()) => {
                let _ = delete_fallback_secret(service, account);
                Ok(())
            }
            Err(_) => save_fallback_secret(service, account, api_key),
        }
    } else {
        save_fallback_secret(service, account, api_key)
    }
}
