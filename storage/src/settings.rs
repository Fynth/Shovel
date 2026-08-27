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
const LM_KEYRING_ACCOUNT: &str = "default";

/// Keyring service name for a catalog provider (`shovel.lm.<provider_id>`).
pub fn lm_service_name(provider_id: &str) -> String {
    format!("shovel.lm.{provider_id}")
}

pub async fn load_app_ui_settings() -> Result<AppUiSettings, String> {
    let mut settings: AppUiSettings = read_json_file(app_ui_settings_path()).await?;
    settings.migrate_legacy_ai_fields();
    Ok(settings)
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

/// Load an LM API key for an arbitrary keyring service name.
///
/// Prefers the system keyring, then the local fallback secret store.
pub async fn load_lm_api_key(service: &str) -> Result<String, String> {
    let service = service.to_string();
    tokio::task::spawn_blocking(move || load_api_key_sync(&service, LM_KEYRING_ACCOUNT))
        .await
        .map_err(|err| format!("failed to join LM secret load task: {err}"))?
}

/// Save an LM API key for an arbitrary keyring service name.
///
/// On keyring failure, writes the fallback store and still returns `Ok(())`
/// when the fallback succeeds — callers treat hard errors as post-JSON warnings.
pub async fn save_lm_api_key(service: &str, api_key: String) -> Result<(), String> {
    let service = service.to_string();
    tokio::task::spawn_blocking(move || save_api_key_sync(&service, LM_KEYRING_ACCOUNT, &api_key))
        .await
        .map_err(|err| format!("failed to join LM secret save task: {err}"))?
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

#[cfg(test)]
mod tests {
    #[test]
    fn lm_keyring_service_name_is_stable() {
        assert_eq!(super::lm_service_name("deepseek"), "shovel.lm.deepseek");
        assert_eq!(super::lm_service_name("custom:abc"), "shovel.lm.custom:abc");
    }
}
