//! Global, runtime-wide application state for the Shovel UI.
//!
//! Most state lives in Dioxus [`GlobalSignal`]s that are read
//! directly from any component without prop-drilling. The module
//! is intentionally split into:
//!
//! - This file: connection sessions, theme / settings, the explorer
//!   cache, the toast queue, and other small global signals.
//! - [`context_menu`]: the right-click menu state machine — items,
//!   callbacks, viewport-aware positioning.
//!
//! The general rule is: any state that needs to be observed by
//! more than one screen lives here, in a global signal. Anything
//! that is purely local to a component or a sub-tree stays in a
//! `use_signal` / `use_resource` inside that component.
//!
//! Persistence is owned by the `storage` crate; this module only
//! triggers saves (for example, on settings change) — it never
//! touches the disk itself.
//!
//! Cross-process invariants worth knowing:
//! - [`APP_STATE`] is the single source of truth for live connection
//!   sessions. Adding or removing a session always goes through
//!   [`add_session`] / [`remove_session`] so that the SSH tunnel
//!   registry and the on-disk session state stay in lockstep.
//! - [`theme`] is the only signal that re-themes the CSS variables.
//!   Components read it via `use_signal` consumers; do not cache it
//!   locally.

use dioxus::prelude::*;
use models::{
    AppState,
    AppThemePreference,
    AppUiSettings,
    ConnectionRequest,
    ConnectionSession,
    DatabaseConnection,
    SqlFormatSettings,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub mod context_menu;

// Explorer cache: session_id -> sections (valid for 5 minutes)
const EXPLORER_CACHE_TTL: Duration = Duration::from_secs(300);

static EXPLORER_CACHE: std::sync::LazyLock<Arc<RwLock<HashMap<u64, ExplorerCacheEntry>>>> =
    std::sync::LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));
static LAST_SESSION_PERSIST_ERROR: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

#[derive(Clone, Debug)]
pub struct ExplorerCacheEntry {
    pub sections: Vec<crate::screens::workspace::ExplorerConnectionSection>,
    pub timestamp: std::time::Instant,
}

impl ExplorerCacheEntry {
    fn is_expired(&self) -> bool {
        self.timestamp.elapsed() > EXPLORER_CACHE_TTL
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppTooltip {
    pub label: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppToast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

pub static APP_STATE: GlobalSignal<AppState> = Signal::global(AppState::default);
pub static APP_THEME: GlobalSignal<String> =
    Signal::global(|| AppThemePreference::Dark.css_class().to_string());
pub static APP_UI_SETTINGS: GlobalSignal<AppUiSettings> = Signal::global(AppUiSettings::default);
pub static APP_SQL_FORMAT_SETTINGS: GlobalSignal<SqlFormatSettings> =
    Signal::global(SqlFormatSettings::default);
pub static APP_AI_FEATURES_ENABLED: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().ai_features_enabled);
pub static APP_READ_ONLY_MODE: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().read_only_mode);
pub static APP_SHOW_SAVED_QUERIES: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_saved_queries);
pub static APP_SHOW_CONNECTIONS: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_connections);
pub static APP_SHOW_EXPLORER: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_explorer);
pub static APP_SHOW_HISTORY: GlobalSignal<bool> = Signal::global(|| false);
pub static APP_SHOW_SQL_EDITOR: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_sql_editor);
pub static APP_SHOW_AGENT_PANEL: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_agent_panel);
pub static APP_SHOW_SETTINGS_MODAL: GlobalSignal<bool> = Signal::global(|| false);
pub static APP_TOOLTIP: GlobalSignal<Option<AppTooltip>> = Signal::global(|| None);
pub static APP_TOAST: GlobalSignal<Vec<AppToast>> = Signal::global(Vec::new);
static NEXT_TOAST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static TOAST_CANCEL_TOKENS: std::sync::LazyLock<Mutex<HashMap<u64, CancellationToken>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn replace_ui_settings(settings: AppUiSettings) {
    *APP_UI_SETTINGS.write() = settings.clone();
    sync_runtime_ui_settings(&settings);
}

pub fn update_ui_settings(update: impl FnOnce(&mut AppUiSettings)) {
    let settings = {
        let mut current = APP_UI_SETTINGS.write();
        update(&mut current);
        current.clone()
    };
    sync_runtime_ui_settings(&settings);
}

pub fn reset_ui_settings() {
    replace_ui_settings(AppUiSettings::default());
    // Purge API keys from keyring so they don't resurrect on next load.
    spawn(async move {
        let _ = services::save_codestral_api_key(String::new()).await;
        let _ = services::save_deepseek_api_key(String::new()).await;
    });
}

pub fn set_theme_preference(theme: AppThemePreference) {
    update_ui_settings(|current| {
        current.theme = theme;
    });
}

pub fn set_ai_features_enabled(enabled: bool) {
    update_ui_settings(|current| {
        current.ai_features_enabled = enabled;
        if !enabled {
            current.show_agent_panel = false;
        }
    });
}

pub fn set_restore_session_on_launch(enabled: bool) {
    update_ui_settings(|current| {
        current.restore_session_on_launch = enabled;
    });
}

pub fn set_read_only_mode(enabled: bool) {
    update_ui_settings(|current| {
        current.read_only_mode = enabled;
    });
}

pub fn set_show_saved_queries(visible: bool) {
    update_ui_settings(|current| {
        current.show_saved_queries = visible;
    });
}

pub fn set_show_connections(visible: bool) {
    update_ui_settings(|current| {
        current.show_connections = visible;
    });
}

pub fn set_show_explorer(visible: bool) {
    update_ui_settings(|current| {
        current.show_explorer = visible;
    });
}

pub fn set_show_history(visible: bool) {
    update_ui_settings(|current| {
        current.show_history = visible;
    });
}

pub fn set_show_sql_editor(visible: bool) {
    update_ui_settings(|current| {
        current.show_sql_editor = visible;
    });
}

pub fn set_show_agent_panel(visible: bool) {
    update_ui_settings(|current| {
        current.show_agent_panel = visible;
    });
}

pub fn set_default_page_size(page_size: u32) {
    update_ui_settings(|current| {
        current.default_page_size = page_size;
    });
}

pub fn set_codestral_enabled(enabled: bool) {
    update_ui_settings(|current| {
        current.codestral.enabled = enabled;
    });
}

pub fn set_codestral_api_key(api_key: String) {
    update_ui_settings(|current| {
        current.codestral.api_key = api_key;
        if current.codestral.api_key.trim().is_empty() {
            current.codestral.enabled = false;
        }
    });
}

pub fn set_codestral_model(model: String) {
    update_ui_settings(|current| {
        current.codestral.model = model;
    });
}

pub fn set_deepseek_enabled(enabled: bool) {
    update_ui_settings(|current| {
        current.deepseek.enabled = enabled;
    });
}

pub fn set_deepseek_api_key(api_key: String) {
    update_ui_settings(|current| {
        current.deepseek.api_key = api_key;
        if current.deepseek.api_key.trim().is_empty() {
            current.deepseek.enabled = false;
        }
    });
}

pub fn set_deepseek_base_url(base_url: String) {
    update_ui_settings(|current| {
        current.deepseek.base_url = base_url;
    });
}

pub fn set_deepseek_model(model: String) {
    update_ui_settings(|current| {
        current.deepseek.model = model;
    });
}

pub fn set_deepseek_thinking_enabled(enabled: bool) {
    update_ui_settings(|current| {
        current.deepseek.thinking_enabled = enabled;
    });
}

pub fn set_deepseek_reasoning_effort(reasoning_effort: String) {
    update_ui_settings(|current| {
        current.deepseek.reasoning_effort = reasoning_effort;
    });
}

fn sync_runtime_ui_settings(settings: &AppUiSettings) {
    *APP_THEME.write() = settings.theme.css_class().to_string();
    *APP_AI_FEATURES_ENABLED.write() = settings.ai_features_enabled;
    *APP_READ_ONLY_MODE.write() = settings.read_only_mode;
    *APP_SHOW_SAVED_QUERIES.write() = settings.show_saved_queries;
    *APP_SHOW_CONNECTIONS.write() = settings.show_connections;
    *APP_SHOW_EXPLORER.write() = settings.show_explorer;
    *APP_SHOW_HISTORY.write() = settings.show_history;
    *APP_SHOW_SQL_EDITOR.write() = settings.show_sql_editor;
    *APP_SHOW_AGENT_PANEL.write() = settings.ai_features_enabled && settings.show_agent_panel;
}

pub fn open_settings_modal() {
    *APP_SHOW_SETTINGS_MODAL.write() = true;
}

pub fn close_settings_modal() {
    *APP_SHOW_SETTINGS_MODAL.write() = false;
}

pub fn show_tooltip(label: String, x: f64, y: f64) {
    *APP_TOOLTIP.write() = Some(AppTooltip { label, x, y });
}

pub fn hide_tooltip() {
    *APP_TOOLTIP.write() = None;
}

pub fn show_toast(message: impl Into<String>, kind: ToastKind) {
    let id = NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let toast = AppToast {
        id,
        message: message.into(),
        kind,
    };
    APP_TOAST.with_mut(|toasts| {
        toasts.push(toast);
    });
    let toast_id = id;
    let cancel_token = CancellationToken::new();
    {
        let mut tokens = TOAST_CANCEL_TOKENS
            .lock()
            .expect("TOAST_CANCEL_TOKENS lock poisoned");
        tokens.insert(toast_id, cancel_token.clone());
    }
    spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                dismiss_toast(toast_id);
            }
            _ = cancel_token.cancelled() => {}
        }
    });
}

pub fn dismiss_toast(id: u64) {
    // Cancel any in-flight auto-dismiss timer for this toast.
    if let Ok(mut tokens) = TOAST_CANCEL_TOKENS.lock()
        && let Some(token) = tokens.remove(&id)
    {
        token.cancel();
    }
    APP_TOAST.with_mut(|toasts| {
        toasts.retain(|t| t.id != id);
    });
}

pub fn toast_error(message: impl Into<String>) {
    show_toast(message, ToastKind::Error);
}

pub fn open_connection_screen() {
    APP_STATE.with_mut(|state| {
        state.show_connection_screen = true;
    });
}

pub fn show_workspace() {
    APP_STATE.with_mut(|state| {
        state.show_connection_screen = false;
    });
}

pub fn activate_session(session_id: u64) {
    APP_STATE.with_mut(|state| {
        if state
            .sessions
            .iter()
            .any(|session| session.id == session_id)
        {
            state.active_session_id = Some(session_id);
            state.show_connection_screen = false;
        }
    });
    persist_session_state();
}

pub fn session_connection(session_id: u64) -> Option<DatabaseConnection> {
    APP_STATE.read().session_connection(session_id).cloned()
}

pub fn add_connection_session(request: ConnectionRequest, connection: DatabaseConnection) -> u64 {
    let session_name = request.display_name();
    let session_kind = request.kind();
    let session_key = request.identity_key();

    let mut activated_id = 0;
    APP_STATE.with_mut(|state| {
        if let Some(existing_session) = state
            .sessions
            .iter_mut()
            .find(|session| session.request.identity_key() == session_key)
        {
            existing_session.request = request.clone();
            existing_session.connection = connection.clone();
            existing_session.name = session_name.clone();
            existing_session.kind = session_kind;
            activated_id = existing_session.id;
        } else {
            let session_id = state.next_session_id;
            state.next_session_id += 1;
            state.sessions.push(ConnectionSession {
                id: session_id,
                name: session_name,
                kind: session_kind,
                request,
                connection,
            });
            activated_id = session_id;
        }

        state.active_session_id = Some(activated_id);
        state.show_connection_screen = false;
    });

    persist_session_state();

    activated_id
}

pub fn remove_session(session_id: u64) {
    APP_STATE.with_mut(|state| {
        let removed_keys = state
            .sessions
            .iter()
            .filter(|session| session.id == session_id)
            .map(|session| session.request.identity_key())
            .collect::<Vec<_>>();

        state.sessions.retain(|session| session.id != session_id);

        if state.active_session_id == Some(session_id) {
            state.active_session_id = state.sessions.first().map(|session| session.id);
        }

        if state.sessions.is_empty() {
            state.active_session_id = None;
            state.show_connection_screen = true;
        }

        for key in removed_keys {
            services::release_ssh_tunnel(&key);
        }
    });
    persist_session_state();
}

pub fn restore_connection_sessions(
    restored: Vec<(ConnectionRequest, DatabaseConnection)>,
    active_name: Option<String>,
) {
    // First collect existing session names and release SSH tunnels
    let existing_keys = {
        let state = APP_STATE.read();
        state
            .sessions
            .iter()
            .map(|session| session.request.identity_key())
            .collect::<Vec<_>>()
    };

    // Release SSH tunnels outside the lock to avoid potential deadlocks
    for key in existing_keys {
        services::release_ssh_tunnel(&key);
    }

    // Now replace sessions atomically
    APP_STATE.with_mut(|state| {
        let mut new_sessions = Vec::with_capacity(restored.len());
        let mut next_id = 1;

        for (request, connection) in restored {
            let session_name = request.display_name();
            let session_kind = request.kind();
            new_sessions.push(ConnectionSession {
                id: next_id,
                name: session_name,
                kind: session_kind,
                request,
                connection,
            });
            next_id += 1;
        }

        state.sessions = new_sessions;
        state.next_session_id = next_id;
        state.active_session_id = active_name
            .as_deref()
            .and_then(|active_name| {
                state
                    .sessions
                    .iter()
                    .find(|session| {
                        session.request.identity_key() == active_name || session.name == active_name
                    })
                    .map(|session| session.id)
            })
            .or_else(|| state.sessions.first().map(|session| session.id));
        state.show_connection_screen = state.sessions.is_empty();
    });

    persist_session_state();
}

fn persist_session_state() {
    let (open_requests, active_connection_name) = {
        let state = APP_STATE.read();
        let requests = state
            .sessions
            .iter()
            .map(|session| session.request.clone())
            .collect::<Vec<_>>();
        let active = state
            .active_session_id
            .and_then(|active_id| state.session(active_id))
            .map(|session| session.request.identity_key());
        (requests, active)
    };

    // Offload synchronous file I/O to a blocking thread so we don't stall the
    // Dioxus render thread.
    spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            services::save_session_state_sync(open_requests, active_connection_name)
        })
        .await;

        match result {
            Ok(Ok(())) =>
                if let Ok(mut last_error) = LAST_SESSION_PERSIST_ERROR.lock() {
                    *last_error = None;
                },
            Ok(Err(err)) => {
                eprintln!("Failed to persist session state: {}", err);
                let should_toast = if let Ok(mut last_error) = LAST_SESSION_PERSIST_ERROR.lock() {
                    if last_error.as_ref() == Some(&err) {
                        false
                    } else {
                        *last_error = Some(err.clone());
                        true
                    }
                } else {
                    true
                };

                if should_toast {
                    toast_error(format!("Failed to save session state: {err}"));
                }
            }
            Err(join_err) => {
                let err = join_err.to_string();
                eprintln!("Failed to persist session state: {}", err);
                if let Ok(mut last_error) = LAST_SESSION_PERSIST_ERROR.lock()
                    && last_error.as_ref() != Some(&err)
                {
                    *last_error = Some(err.clone());
                    toast_error(format!("Failed to save session state: {err}"));
                }
            }
        }
    });
}

// Explorer cache functions
pub async fn get_cached_explorer(
    session_id: u64,
) -> Option<Vec<crate::screens::workspace::ExplorerConnectionSection>> {
    let cache = EXPLORER_CACHE.read().await;
    cache.get(&session_id).and_then(|entry| {
        if entry.is_expired() {
            None
        } else {
            Some(entry.sections.clone())
        }
    })
}

pub async fn cache_explorer(
    session_id: u64,
    sections: Vec<crate::screens::workspace::ExplorerConnectionSection>,
) {
    let mut cache = EXPLORER_CACHE.write().await;
    cache.insert(
        session_id,
        ExplorerCacheEntry {
            sections,
            timestamp: std::time::Instant::now(),
        },
    );
}
