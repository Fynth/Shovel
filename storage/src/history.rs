use keyring::{Entry, Error as KeyringError};
use models::{
    ClickHouseFormData, ConnectionRequest, MySqlFormData, PostgresFormData, QueryHistoryItem,
    SavedConnection, SqliteFormData, SshTunnelConfig,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{
    fs_store::{read_text_file, saved_connections_path, session_state_path, write_json_file},
    secrets::{delete_fallback_secret, load_fallback_secret, save_fallback_secret},
};

const MAX_SAVED_CONNECTIONS: usize = 10;
const KEYRING_SERVICE: &str = "shovel.connections";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedSavedConnection {
    name: String,
    request: PersistedConnectionRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PersistedConnectionRequest {
    Sqlite(SqliteFormData),
    Postgres(PostgresConnectionMetadata),
    MySql(MySqlConnectionMetadata),
    ClickHouse(ClickHouseConnectionMetadata),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PostgresConnectionMetadata {
    host: String,
    port: u16,
    username: String,
    database: String,
    #[serde(default)]
    ssl_mode: String,
    #[serde(default)]
    ssh_tunnel: Option<SshTunnelConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MySqlConnectionMetadata {
    host: String,
    port: u16,
    username: String,
    database: String,
    #[serde(default)]
    ssl_mode: String,
    #[serde(default)]
    ssh_tunnel: Option<SshTunnelConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ClickHouseConnectionMetadata {
    host: String,
    port: u16,
    username: String,
    database: String,
    #[serde(default)]
    ssh_tunnel: Option<SshTunnelConfig>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedSessionState {
    #[serde(default)]
    open_connections: Vec<PersistedSavedConnection>,
    active_connection_name: Option<String>,
    #[serde(default)]
    tab_drafts: Vec<models::TabDraft>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct LegacySessionState {
    #[serde(default)]
    open_requests: Vec<ConnectionRequest>,
    active_connection_name: Option<String>,
    #[serde(default)]
    tab_drafts: Vec<models::TabDraft>,
}

/// Load all saved connections from disk, hydrating secrets from the keyring.
///
/// Reads `saved_connections.json` and resolves each connection's password
/// (or other secret) from the system keyring or the fallback secret store.
/// Supports legacy formats and migrates them forward automatically.
///
/// # Errors
///
/// Returns an error string if the file cannot be read or parsed.
pub async fn load_saved_connections() -> Result<Vec<SavedConnection>, String> {
    let path = saved_connections_path();
    let Some(content) = read_text_file(&path).await? else {
        return Ok(Vec::new());
    };
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(persisted) = serde_json::from_str::<Vec<PersistedSavedConnection>>(&content) {
        return hydrate_saved_connections(persisted);
    }

    let legacy = serde_json::from_str::<Vec<SavedConnection>>(&content)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    persist_saved_connections(&legacy, &[]).await?;
    Ok(legacy
        .into_iter()
        .map(|saved_connection| SavedConnection {
            name: saved_connection.request.display_name(),
            request: saved_connection.request,
        })
        .collect())
}

/// Persist a [`ConnectionRequest`] as a saved connection.
///
/// The request is upserted into the saved-connections list (keyed by
/// identity key). The list is capped at [`MAX_SAVED_CONNECTIONS`] entries.
/// Connection secrets are stored in the system keyring.
///
/// # Arguments
///
/// * `request` - The connection request to save.
///
/// # Errors
///
/// Returns an error string if writing the JSON file or the keyring entry fails.
pub async fn save_connection_request(request: ConnectionRequest) -> Result<(), String> {
    let mut saved_connections = load_saved_connections().await.unwrap_or_default();
    let previous_connections = saved_connections.clone();
    upsert_saved_connection(&mut saved_connections, request, None);

    persist_saved_connections(&saved_connections, &previous_connections).await
}

/// Replace an existing saved connection identified by its previous identity key.
///
/// This is used when editing a connection: the old entry (matched by
/// `previous_identity_key`) is removed and the new `request` takes its place.
/// If no entry with the given key exists, the new request is simply appended.
///
/// # Arguments
///
/// * `previous_identity_key` - The identity key of the connection to replace.
/// * `request` - The new connection request to store.
///
/// # Errors
///
/// Returns an error string if writing the JSON file or the keyring entry fails.
pub async fn replace_connection_request(
    previous_identity_key: String,
    request: ConnectionRequest,
) -> Result<(), String> {
    let mut saved_connections = load_saved_connections().await.unwrap_or_default();
    let previous_connections = saved_connections.clone();
    upsert_saved_connection(
        &mut saved_connections,
        request,
        Some(previous_identity_key.as_str()),
    );

    persist_saved_connections(&saved_connections, &previous_connections).await
}

/// Load recent query history from the SQLite-backed store.
///
/// Initializes the [`QueryHistoryStore`] schema (creating tables and
/// migrating legacy JSON data if needed), then returns the 20 most
/// recent history items.
///
/// # Errors
///
/// Returns an error string if schema initialization or the query fails.
pub async fn load_query_history() -> Result<Vec<QueryHistoryItem>, String> {
    crate::query_history::QueryHistoryStore::init().await?;
    crate::query_history::QueryHistoryStore::load(20).await
}

/// Append a single [`QueryHistoryItem`] to the query history store.
///
/// Initializes the schema if needed, inserts the item, and trims the
/// store to the configured maximum size.
///
/// # Arguments
///
/// * `item` - The history item to persist.
///
/// # Errors
///
/// Returns an error string if the insert fails.
pub async fn append_query_history(item: QueryHistoryItem) -> Result<(), String> {
    crate::query_history::QueryHistoryStore::init().await?;
    crate::query_history::QueryHistoryStore::save(&item).await
}

/// Persist the current session state to disk asynchronously.
///
/// Writes `session_state.json` with the list of open connections and
/// the name of the currently active connection (if any). Secrets for
/// each connection are stored in the keyring.
///
/// # Arguments
///
/// * `open_requests` - All currently open connection requests.
/// * `active_connection_name` - The name of the active connection tab, if any.
///
/// # Errors
///
/// Returns an error string if writing the file or any keyring entry fails.
pub async fn save_session_state(
    open_requests: Vec<ConnectionRequest>,
    active_connection_name: Option<String>,
    tab_drafts: Vec<models::TabDraft>,
) -> Result<(), String> {
    let (state, secret_errors) =
        build_persisted_session_state(open_requests, active_connection_name, tab_drafts);
    write_json_file(session_state_path(), &state)
        .await
        .and_then(|_| finalize_secret_errors("session state", secret_errors))
}

/// Load the persisted session state asynchronously.
///
/// Reads `session_state.json` and hydrates each connection's secret from
/// the keyring. Supports both the current and legacy session-state formats.
///
/// # Returns
///
/// A tuple of `(open_requests, active_connection_name)`. If no session file
/// exists, both values are empty / `None`.
///
/// # Errors
///
/// Returns an error string if the file cannot be read or parsed.
#[allow(clippy::type_complexity)]
pub async fn load_session_state() -> Result<
    (
        Vec<ConnectionRequest>,
        Option<String>,
        Vec<models::TabDraft>,
    ),
    String,
> {
    let state = read_session_state_async(session_state_path()).await?;
    let active_connection_name =
        normalize_active_connection_name(&state.open_connections, state.active_connection_name);
    let open_requests = hydrate_session_requests(state.open_connections)?;
    Ok((open_requests, active_connection_name, state.tab_drafts))
}

/// Persist the current session state to disk synchronously.
///
/// Synchronous variant of [`save_session_state`]. Writes `session_state.json`
/// with the list of open connections and the name of the currently active
/// connection (if any). Secrets for each connection are stored in the keyring.
///
/// # Arguments
///
/// * `open_requests` - All currently open connection requests.
/// * `active_connection_name` - The name of the active connection tab, if any.
///
/// # Errors
///
/// Returns an error string if writing the file or any keyring entry fails.
pub fn save_session_state_sync(
    open_requests: Vec<ConnectionRequest>,
    active_connection_name: Option<String>,
    tab_drafts: Vec<models::TabDraft>,
) -> Result<(), String> {
    let (state, secret_errors) =
        build_persisted_session_state(open_requests, active_connection_name, tab_drafts);
    write_session_state_sync(session_state_path(), &state)
        .and_then(|_| finalize_secret_errors("session state", secret_errors))
}

/// Load the persisted session state synchronously.
///
/// Synchronous variant of [`load_session_state`]. Reads `session_state.json`
/// and hydrates each connection's secret from the keyring. Supports both the
/// current and legacy session-state formats.
///
/// # Returns
///
/// A tuple of `(open_requests, active_connection_name)`. If no session file
/// exists, both values are empty / `None`.
///
/// # Errors
///
/// Returns an error string if the file cannot be read or parsed.
#[allow(clippy::type_complexity)]
pub fn load_session_state_sync() -> Result<
    (
        Vec<ConnectionRequest>,
        Option<String>,
        Vec<models::TabDraft>,
    ),
    String,
> {
    let state = read_session_state_sync(session_state_path())?;
    let active_connection_name =
        normalize_active_connection_name(&state.open_connections, state.active_connection_name);
    let open_requests = hydrate_session_requests(state.open_connections)?;
    Ok((open_requests, active_connection_name, state.tab_drafts))
}

async fn persist_saved_connections(
    saved_connections: &[SavedConnection],
    previous_connections: &[SavedConnection],
) -> Result<(), String> {
    let mut secret_errors = Vec::new();

    for saved_connection in saved_connections {
        if let Err(err) = sync_connection_secret(saved_connection) {
            secret_errors.push(err);
        }
    }

    for removed_connection in previous_connections {
        if !saved_connections
            .iter()
            .any(|saved| saved.request.identity_key() == removed_connection.request.identity_key())
            && let Err(err) =
                delete_connection_secret(&removed_connection.name, &removed_connection.request)
        {
            secret_errors.push(err);
        }
    }

    let persisted = saved_connections
        .iter()
        .cloned()
        .map(to_persisted_connection)
        .collect::<Vec<_>>();
    write_json_file(saved_connections_path(), &persisted).await?;

    if secret_errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "saved connection metadata, but secure storage had issues: {}",
            secret_errors.join("; ")
        ))
    }
}

fn upsert_saved_connection(
    saved_connections: &mut Vec<SavedConnection>,
    request: ConnectionRequest,
    replaced_identity_key: Option<&str>,
) {
    if let Some(previous_identity_key) = replaced_identity_key {
        saved_connections.retain(|saved| saved.request.identity_key() != previous_identity_key);
    }

    let request_key = request.identity_key();
    saved_connections.retain(|saved| saved.request.identity_key() != request_key);
    saved_connections.insert(
        0,
        SavedConnection {
            name: request.display_name(),
            request,
        },
    );
    if saved_connections.len() > MAX_SAVED_CONNECTIONS {
        saved_connections.truncate(MAX_SAVED_CONNECTIONS);
    }
}

fn hydrate_saved_connections(
    persisted: Vec<PersistedSavedConnection>,
) -> Result<Vec<SavedConnection>, String> {
    let mut restored = Vec::with_capacity(persisted.len());

    for saved_connection in persisted {
        restored.push(hydrate_saved_connection(saved_connection)?);
    }

    Ok(restored)
}

fn hydrate_saved_connection(
    saved_connection: PersistedSavedConnection,
) -> Result<SavedConnection, String> {
    let request_without_password = persisted_request_without_password(&saved_connection.request);
    let password = load_connection_secret(&saved_connection.name, &request_without_password)
        .ok()
        .flatten();
    let request = persisted_request_with_password(saved_connection.request, password);

    Ok(SavedConnection {
        name: request.display_name(),
        request,
    })
}

fn to_persisted_connection(saved_connection: SavedConnection) -> PersistedSavedConnection {
    let request = match saved_connection.request {
        ConnectionRequest::Sqlite(data) => PersistedConnectionRequest::Sqlite(data),
        ConnectionRequest::Postgres(data) => {
            PersistedConnectionRequest::Postgres(PostgresConnectionMetadata {
                host: data.host,
                port: data.port,
                username: data.username,
                database: data.database,
                ssl_mode: data.ssl_mode,
                ssh_tunnel: data.ssh_tunnel,
            })
        }
        ConnectionRequest::MySql(data) => {
            PersistedConnectionRequest::MySql(MySqlConnectionMetadata {
                host: data.host,
                port: data.port,
                username: data.username,
                database: data.database,
                ssl_mode: data.ssl_mode,
                ssh_tunnel: data.ssh_tunnel,
            })
        }
        ConnectionRequest::ClickHouse(data) => {
            PersistedConnectionRequest::ClickHouse(ClickHouseConnectionMetadata {
                host: data.host,
                port: data.port,
                username: data.username,
                database: data.database,
                ssh_tunnel: data.ssh_tunnel,
            })
        }
    };

    PersistedSavedConnection {
        name: saved_connection.name,
        request,
    }
}

fn sync_connection_secret(saved_connection: &SavedConnection) -> Result<(), String> {
    match &saved_connection.request {
        ConnectionRequest::Sqlite(_) => {
            delete_connection_secret(&saved_connection.name, &saved_connection.request)?;
        }
        ConnectionRequest::Postgres(data) => {
            store_connection_secret(saved_connection, &data.password)?;
        }
        ConnectionRequest::MySql(data) => {
            store_connection_secret(saved_connection, &data.password)?;
        }
        ConnectionRequest::ClickHouse(data) => {
            store_connection_secret(saved_connection, &data.password)?;
        }
    }

    Ok(())
}

fn store_connection_secret(saved_connection: &SavedConnection, secret: &str) -> Result<(), String> {
    let current_key = saved_connection.request.identity_key();
    if secret.is_empty() {
        delete_secret(&current_key)?;
    } else {
        store_secret(&current_key, secret)?;
    }

    let legacy_name = saved_connection.name.trim();
    if !legacy_name.is_empty() && legacy_name != current_key {
        delete_secret(legacy_name)?;
    }

    Ok(())
}

fn load_connection_secret(
    legacy_name: &str,
    request: &ConnectionRequest,
) -> Result<Option<String>, String> {
    let current_key = request.identity_key();
    if let Some(secret) = load_secret(&current_key)? {
        let legacy_name = legacy_name.trim();
        if !legacy_name.is_empty() && legacy_name != current_key {
            let _ = delete_secret(legacy_name);
        }
        return Ok(Some(secret));
    }

    let legacy_name = legacy_name.trim();
    if legacy_name.is_empty() || legacy_name == current_key {
        return Ok(None);
    }

    let Some(secret) = load_secret(legacy_name)? else {
        return Ok(None);
    };

    store_secret(&current_key, &secret)?;
    let _ = delete_secret(legacy_name);
    Ok(Some(secret))
}

fn delete_connection_secret(legacy_name: &str, request: &ConnectionRequest) -> Result<(), String> {
    let current_key = request.identity_key();
    delete_secret(&current_key)?;

    let legacy_name = legacy_name.trim();
    if !legacy_name.is_empty() && legacy_name != current_key {
        delete_secret(legacy_name)?;
    }

    Ok(())
}

fn store_secret(connection_name: &str, secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        return delete_secret(connection_name);
    }

    match secret_entry(connection_name) {
        Ok(entry) => match entry.set_password(secret) {
            Ok(()) => {
                let _ = delete_fallback_secret(KEYRING_SERVICE, connection_name);
                Ok(())
            }
            Err(_) => save_fallback_secret(KEYRING_SERVICE, connection_name, secret),
        },
        Err(_) => save_fallback_secret(KEYRING_SERVICE, connection_name, secret),
    }
}

fn load_secret(connection_name: &str) -> Result<Option<String>, String> {
    match secret_entry(connection_name) {
        Ok(entry) => match entry.get_password() {
            Ok(secret) => {
                let _ = delete_fallback_secret(KEYRING_SERVICE, connection_name);
                Ok(Some(secret))
            }
            Err(KeyringError::NoEntry) => load_fallback_secret(KEYRING_SERVICE, connection_name),
            Err(_) => load_fallback_secret(KEYRING_SERVICE, connection_name),
        },
        Err(_) => load_fallback_secret(KEYRING_SERVICE, connection_name),
    }
}

fn delete_secret(connection_name: &str) -> Result<(), String> {
    match secret_entry(connection_name) {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => {
                delete_fallback_secret(KEYRING_SERVICE, connection_name)
            }
            Err(_) => delete_fallback_secret(KEYRING_SERVICE, connection_name),
        },
        Err(_) => delete_fallback_secret(KEYRING_SERVICE, connection_name),
    }
}

fn secret_entry(connection_name: &str) -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, &secret_key(connection_name)).map_err(|err| {
        format!("failed to create secure storage entry for {connection_name}: {err}")
    })
}

fn secret_key(connection_name: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(connection_name.as_bytes());
    let result = hasher.finalize();
    // Use first 6 bytes (48 bits → 12 hex chars), enough for keyring uniqueness
    let short = u64::from_be_bytes(result[..8].try_into().unwrap()) & 0xffffffffffff;
    format!("connection-{short:012x}")
}

fn persisted_request_without_password(request: &PersistedConnectionRequest) -> ConnectionRequest {
    match request {
        PersistedConnectionRequest::Sqlite(data) => ConnectionRequest::Sqlite(data.clone()),
        PersistedConnectionRequest::Postgres(data) => {
            ConnectionRequest::Postgres(PostgresFormData {
                host: data.host.clone(),
                port: data.port,
                username: data.username.clone(),
                password: String::new(),
                database: data.database.clone(),
                ssl_mode: if data.ssl_mode.is_empty() {
                    "prefer".to_string()
                } else {
                    data.ssl_mode.clone()
                },
                ssh_tunnel: data.ssh_tunnel.clone(),
            })
        }
        PersistedConnectionRequest::MySql(data) => ConnectionRequest::MySql(MySqlFormData {
            host: data.host.clone(),
            port: data.port,
            username: data.username.clone(),
            password: String::new(),
            database: data.database.clone(),
            ssl_mode: if data.ssl_mode.is_empty() {
                "preferred".to_string()
            } else {
                data.ssl_mode.clone()
            },
            ssh_tunnel: data.ssh_tunnel.clone(),
        }),
        PersistedConnectionRequest::ClickHouse(data) => {
            ConnectionRequest::ClickHouse(ClickHouseFormData {
                host: data.host.clone(),
                port: data.port,
                username: data.username.clone(),
                password: String::new(),
                database: data.database.clone(),
                ssh_tunnel: data.ssh_tunnel.clone(),
            })
        }
    }
}

fn persisted_request_with_password(
    request: PersistedConnectionRequest,
    password: Option<String>,
) -> ConnectionRequest {
    match request {
        PersistedConnectionRequest::Sqlite(data) => ConnectionRequest::Sqlite(data),
        PersistedConnectionRequest::Postgres(data) => {
            ConnectionRequest::Postgres(PostgresFormData {
                host: data.host,
                port: data.port,
                username: data.username,
                password: password.clone().unwrap_or_default(),
                database: data.database,
                ssl_mode: if data.ssl_mode.is_empty() {
                    "prefer".to_string()
                } else {
                    data.ssl_mode
                },
                ssh_tunnel: data.ssh_tunnel,
            })
        }
        PersistedConnectionRequest::MySql(data) => ConnectionRequest::MySql(MySqlFormData {
            host: data.host,
            port: data.port,
            username: data.username,
            password: password.clone().unwrap_or_default(),
            database: data.database,
            ssl_mode: if data.ssl_mode.is_empty() {
                "preferred".to_string()
            } else {
                data.ssl_mode
            },
            ssh_tunnel: data.ssh_tunnel,
        }),
        PersistedConnectionRequest::ClickHouse(data) => {
            ConnectionRequest::ClickHouse(ClickHouseFormData {
                host: data.host,
                port: data.port,
                username: data.username,
                password: password.unwrap_or_default(),
                database: data.database,
                ssh_tunnel: data.ssh_tunnel,
            })
        }
    }
}

fn normalize_active_connection_name(
    open_connections: &[PersistedSavedConnection],
    active_connection_name: Option<String>,
) -> Option<String> {
    let active_connection_name = active_connection_name?;
    open_connections
        .iter()
        .find(|connection| connection.name == active_connection_name)
        .map(|connection| persisted_request_without_password(&connection.request).identity_key())
        .or(Some(active_connection_name))
}

fn build_persisted_session_state(
    open_requests: Vec<ConnectionRequest>,
    active_connection_name: Option<String>,
    tab_drafts: Vec<models::TabDraft>,
) -> (PersistedSessionState, Vec<String>) {
    let mut secret_errors = Vec::new();
    let open_connections = open_requests
        .into_iter()
        .map(|request| SavedConnection {
            name: request.display_name(),
            request,
        })
        .map(|saved_connection| {
            if let Err(err) = sync_connection_secret(&saved_connection) {
                secret_errors.push(err);
            }
            to_persisted_connection(saved_connection)
        })
        .collect::<Vec<_>>();

    (
        PersistedSessionState {
            open_connections,
            active_connection_name,
            tab_drafts,
        },
        secret_errors,
    )
}

fn hydrate_session_requests(
    open_connections: Vec<PersistedSavedConnection>,
) -> Result<Vec<ConnectionRequest>, String> {
    hydrate_saved_connections(open_connections).map(|saved_connections| {
        saved_connections
            .into_iter()
            .map(|saved_connection| saved_connection.request)
            .collect()
    })
}

async fn read_session_state_async(path: PathBuf) -> Result<PersistedSessionState, String> {
    match read_text_file(&path).await? {
        Some(content) => parse_session_state(&content, &path),
        None => Ok(PersistedSessionState::default()),
    }
}

fn parse_session_state(content: &str, path: &Path) -> Result<PersistedSessionState, String> {
    if content.trim().is_empty() {
        return Ok(PersistedSessionState::default());
    }

    if let Ok(state) = serde_json::from_str::<PersistedSessionState>(content) {
        return Ok(state);
    }

    match serde_json::from_str::<LegacySessionState>(content) {
        Ok(legacy) => Ok(PersistedSessionState {
            open_connections: legacy
                .open_requests
                .into_iter()
                .map(|request| SavedConnection {
                    name: request.display_name(),
                    request,
                })
                .map(to_persisted_connection)
                .collect(),
            active_connection_name: legacy.active_connection_name,
            tab_drafts: legacy.tab_drafts,
        }),
        Err(err) => Err(format!("failed to parse {}: {err}", path.display())),
    }
}

fn finalize_secret_errors(context: &str, secret_errors: Vec<String>) -> Result<(), String> {
    if secret_errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "saved {context}, but secure storage had issues: {}",
            secret_errors.join("; ")
        ))
    }
}

fn read_session_state_sync(path: PathBuf) -> Result<PersistedSessionState, String> {
    match fs::read_to_string(&path) {
        Ok(content) => parse_session_state(&content, &path),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(PersistedSessionState::default()),
        Err(err) => Err(format!("failed to read {}: {err}", path.display())),
    }
}

fn write_session_state_sync(path: PathBuf, value: &PersistedSessionState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create storage dir {}: {err}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(value)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{
        ConnectionRequest, MySqlFormData, PostgresFormData, SavedConnection, SqliteFormData,
    };

    fn sqlite_request(path: &str) -> ConnectionRequest {
        ConnectionRequest::Sqlite(SqliteFormData {
            path: path.to_string(),
        })
    }

    #[test]
    fn upsert_saved_connection_replaces_previous_identity_key() {
        let old_request = sqlite_request("/tmp/old.db");
        let new_request = sqlite_request("/tmp/new.db");
        let mut saved_connections = vec![SavedConnection {
            name: old_request.display_name(),
            request: old_request.clone(),
        }];

        upsert_saved_connection(
            &mut saved_connections,
            new_request.clone(),
            Some(&old_request.identity_key()),
        );

        assert_eq!(saved_connections.len(), 1);
        assert_eq!(saved_connections[0].request, new_request);
    }

    #[test]
    fn upsert_saved_connection_moves_existing_connection_to_front() {
        let first_request = sqlite_request("/tmp/first.db");
        let second_request = sqlite_request("/tmp/second.db");
        let mut saved_connections = vec![
            SavedConnection {
                name: first_request.display_name(),
                request: first_request.clone(),
            },
            SavedConnection {
                name: second_request.display_name(),
                request: second_request.clone(),
            },
        ];

        upsert_saved_connection(&mut saved_connections, first_request.clone(), None);

        assert_eq!(saved_connections.len(), 2);
        assert_eq!(saved_connections[0].request, first_request);
        assert_eq!(saved_connections[1].request, second_request);
    }

    // ── ssl_mode round-trip through session persistence ────────────────
    // Regression: ssl_mode was hardcoded during hydration, discarding the
    // user's chosen SSL mode on every save/restore cycle.

    fn postgres_request(ssl_mode: &str) -> ConnectionRequest {
        ConnectionRequest::Postgres(PostgresFormData {
            host: "db.example.com".to_string(),
            port: 5432,
            username: "pg".to_string(),
            password: String::new(),
            database: "app".to_string(),
            ssl_mode: ssl_mode.to_string(),
            ssh_tunnel: None,
        })
    }

    fn mysql_request(ssl_mode: &str) -> ConnectionRequest {
        ConnectionRequest::MySql(MySqlFormData {
            host: "db.example.com".to_string(),
            port: 3306,
            username: "root".to_string(),
            password: String::new(),
            database: "app".to_string(),
            ssl_mode: ssl_mode.to_string(),
            ssh_tunnel: None,
        })
    }

    #[test]
    fn postgres_ssl_mode_survives_roundtrip_with_password() {
        let conn = SavedConnection {
            name: "pg-prod".to_string(),
            request: postgres_request("require"),
        };
        let persisted = to_persisted_connection(conn);
        let restored = persisted_request_with_password(persisted.request, Some("pw".to_string()));
        match restored {
            ConnectionRequest::Postgres(data) => assert_eq!(data.ssl_mode, "require"),
            other => panic!("expected Postgres, got {other:?}"),
        }
    }

    #[test]
    fn mysql_ssl_mode_survives_roundtrip_without_password() {
        let conn = SavedConnection {
            name: "mysql-prod".to_string(),
            request: mysql_request("verify-identity"),
        };
        let persisted = to_persisted_connection(conn);
        let restored = persisted_request_without_password(&persisted.request);
        match restored {
            ConnectionRequest::MySql(data) => assert_eq!(data.ssl_mode, "verify-identity"),
            other => panic!("expected MySql, got {other:?}"),
        }
    }

    #[test]
    fn legacy_postgres_session_without_ssl_mode_defaults_to_prefer() {
        // Old saved sessions predate the ssl_mode field; #[serde(default)]
        // must fill it in (empty) and hydration must map empty -> "prefer".
        let json = r#"{
            "name":"pg-old",
            "request":{
                "Postgres":{
                    "host":"db.example.com",
                    "port":5432,
                    "username":"pg",
                    "database":"app",
                    "ssh_tunnel":null
                }
            }
        }"#;
        let persisted: PersistedSavedConnection = serde_json::from_str(json).expect("deserialize");
        let restored = persisted_request_without_password(&persisted.request);
        match restored {
            ConnectionRequest::Postgres(data) => assert_eq!(data.ssl_mode, "prefer"),
            other => panic!("expected Postgres, got {other:?}"),
        }
    }

    #[test]
    fn legacy_mysql_session_without_ssl_mode_defaults_to_preferred() {
        let json = r#"{
            "name":"mysql-old",
            "request":{
                "MySql":{
                    "host":"db.example.com",
                    "port":3306,
                    "username":"root",
                    "database":"app",
                    "ssh_tunnel":null
                }
            }
        }"#;
        let persisted: PersistedSavedConnection = serde_json::from_str(json).expect("deserialize");
        let restored = persisted_request_with_password(persisted.request, None);
        match restored {
            ConnectionRequest::MySql(data) => assert_eq!(data.ssl_mode, "preferred"),
            other => panic!("expected MySql, got {other:?}"),
        }
    }
}
