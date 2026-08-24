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
    UiDensity,
    WorkspaceSplitMode,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub mod actions;
pub mod commands;
pub mod context_menu;
pub mod global_search;
pub mod keyboard;

// Re-export the workspace-facing global-search surface so the workspace
// dispatcher can reach it via `crate::app_state::*` instead of
// reaching into the `global_search` sub-module. Mirrors how
// `APP_COMMAND_PALETTE` is exposed today.
pub use global_search::{
    APP_GLOBAL_SEARCH_OBJECTS,
    APP_GLOBAL_SEARCH_REQUEST,
    APP_GLOBAL_SEARCH_REQUEST_KIND,
    APP_GLOBAL_SEARCH_REQUEST_PAYLOAD,
    close_global_search,
    open_global_search_with_snapshots,
};

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
        self.is_expired_at(std::time::Instant::now())
    }

    fn is_expired_at(&self, now: std::time::Instant) -> bool {
        now.duration_since(self.timestamp) > EXPLORER_CACHE_TTL
    }
}

/// Sweep expired entries in place; returns how many were evicted.
/// A full sweep on every insert is cheap: the map holds at most one
/// entry per live session.
fn prune_expired(cache: &mut HashMap<u64, ExplorerCacheEntry>, now: std::time::Instant) -> usize {
    let before = cache.len();
    cache.retain(|_, entry| !entry.is_expired_at(now));
    before - cache.len()
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

/// Краткая сводка о последнем выполненном запросе для статус-бара.
/// Хранится в глобальном сигнале [`APP_LAST_QUERY`], чтобы статус-бар
/// (компонент верхнего уровня) мог отображать тайминг и результат
/// запроса без доступа к локальным сигналам вкладок рабочего пространства.
#[derive(Clone, Debug, PartialEq)]
pub struct LastQuerySummary {
    /// Краткая подпись результата, например "Loaded rows 1-50",
    /// "Rows affected: 3" или "Error".
    pub label: String,
    /// Длительность последнего запроса (мс), если известна.
    pub duration_ms: Option<u64>,
    /// true, если запрос завершился ошибкой.
    pub failed: bool,
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
pub static APP_UI_DENSITY: GlobalSignal<UiDensity> =
    Signal::global(|| AppUiSettings::default().density);
pub static APP_UI_SETTINGS: GlobalSignal<AppUiSettings> = Signal::global(AppUiSettings::default);
pub static APP_SQL_FORMAT_SETTINGS: GlobalSignal<SqlFormatSettings> =
    Signal::global(SqlFormatSettings::default);
pub static APP_AI_FEATURES_ENABLED: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().ai_features_enabled);
pub static APP_AI_AUTO_APPLY_COMPLETIONS: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().ai_auto_apply_completions);
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
pub static APP_SHOW_BOTTOM_PANEL: GlobalSignal<bool> =
    Signal::global(|| AppUiSettings::default().show_bottom_panel);
/// Persisted height of the bottom dock (Output / Messages / Query Log /
/// Transactions / Problems) in CSS pixels. Mirrored from
/// [`AppUiSettings::bottom_panel_height`] so the resize handle can read
/// it without re-reading the full settings struct on every pointer move.
pub static APP_BOTTOM_PANEL_HEIGHT: GlobalSignal<f64> =
    Signal::global(|| AppUiSettings::default().bottom_panel_height);
/// Active-tab body split mode (Off / Horizontal / Vertical). Mirrored
/// from [`AppUiSettings::split_mode`] so the tab body and the workspace
/// toolbar can both read it without re-reading the full settings struct
/// on every render. Equality-guarded in `sync_runtime_ui_settings` so
/// unrelated settings toggles do not invalidate the tab body.
pub static APP_SPLIT_MODE: GlobalSignal<WorkspaceSplitMode> =
    Signal::global(|| AppUiSettings::default().split_mode);
pub static APP_TOOLTIP: GlobalSignal<Option<AppTooltip>> = Signal::global(|| None);
pub static APP_TOAST: GlobalSignal<Vec<AppToast>> = Signal::global(Vec::new);
pub static APP_TAB_DRAFTS: GlobalSignal<Vec<models::TabDraft>> = Signal::global(Vec::new);
/// Most-recently-closed query tabs (newest first). Capped to
/// `RECENTLY_CLOSED_TABS_LIMIT`. Restored by the tab context menu's
/// "Reopen Closed Tab" item so the user can get back the last few
/// tabs after closing them. Tab state is kept in-memory only (the
/// active tab list is also in-memory; the on-disk draft layer is
/// separate and untouched by this stack).
pub static APP_RECENTLY_CLOSED_TABS: GlobalSignal<Vec<models::QueryTabState>> =
    Signal::global(Vec::new);
/// Maximum number of tabs retained for "Reopen Closed Tab".
pub const RECENTLY_CLOSED_TABS_LIMIT: usize = 8;
pub static APP_LAST_QUERY: GlobalSignal<Option<LastQuerySummary>> = Signal::global(|| None);
pub static APP_FOCUS_EDITOR_REQUEST: GlobalSignal<u64> = Signal::global(|| 0);
pub static APP_FOCUS_FILTER_PANEL_REQUEST: GlobalSignal<u64> = Signal::global(|| 0);

/// Push a tab onto the "recently closed" stack. The tab is
/// prepended (newest first) and the stack is capped at
/// [`RECENTLY_CLOSED_TABS_LIMIT`]. The tab's `pinned` flag is
/// cleared on push so the reopened tab is not sticky by default.
pub fn push_recently_closed_tab(tab: models::QueryTabState) {
    APP_RECENTLY_CLOSED_TABS.with_mut(|stack| {
        push_recently_closed_tab_into(stack, tab);
    });
}

/// Pop the most-recently-closed tab. Returns `None` when the stack
/// is empty. The popped tab is removed from the stack and a fresh
/// `id` is assigned so it does not collide with any live tab.
pub fn pop_recently_closed_tab(next_tab_id: &mut Signal<u64>) -> Option<models::QueryTabState> {
    let mut popped: Option<models::QueryTabState> = None;
    APP_RECENTLY_CLOSED_TABS.with_mut(|stack| {
        if !stack.is_empty() {
            popped = Some(stack.remove(0));
        }
    });
    popped.map(|mut tab| {
        let new_id = next_tab_id();
        next_tab_id.set(new_id + 1);
        tab.id = new_id;
        tab
    })
}

/// Pure helper that mutates a stack in place: prepends the tab,
/// strips `pinned`, and caps the stack at
/// [`RECENTLY_CLOSED_TABS_LIMIT`]. Exposed so the behaviour can be
/// unit-tested without spinning up a Dioxus runtime.
fn push_recently_closed_tab_into(
    stack: &mut Vec<models::QueryTabState>,
    mut tab: models::QueryTabState,
) {
    tab.pinned = false;
    stack.insert(0, tab);
    if stack.len() > RECENTLY_CLOSED_TABS_LIMIT {
        stack.truncate(RECENTLY_CLOSED_TABS_LIMIT);
    }
}

// Command-palette visibility. The palette is a compact overlay that
// can be opened from anywhere with Ctrl+Shift+P (or the palette's own
// "Open Command Palette" entry). See `commands.rs` for the catalog
// and `components::command_palette` for the renderer.
pub static APP_COMMAND_PALETTE: GlobalSignal<bool> = Signal::global(|| false);

// Bumped when the palette dispatches a command that needs workspace
// context (run query, format, explain, new/close/next tab, save query,
// refresh explorer). The workspace watches this counter and reacts in
// a `use_effect`, keeping the catalog in `commands.rs` free of
// workspace-local signals.
pub static APP_COMMAND_REQUEST: GlobalSignal<u64> = Signal::global(|| 0);
pub static APP_COMMAND_REQUEST_KIND: GlobalSignal<u64> = Signal::global(|| 0);

pub fn request_focus_editor() {
    let mut counter = APP_FOCUS_EDITOR_REQUEST.write();
    *counter = counter.wrapping_add(1);
}

pub fn request_focus_filter_panel() {
    let mut counter = APP_FOCUS_FILTER_PANEL_REQUEST.write();
    *counter = counter.wrapping_add(1);
}

pub fn open_command_palette() {
    *APP_COMMAND_PALETTE.write() = true;
}

pub fn close_command_palette() {
    if APP_COMMAND_PALETTE() {
        *APP_COMMAND_PALETTE.write() = false;
    }
}

/// Bump the workspace-scoped command request counter with a stable
/// discriminator (`kind`) so the workspace can route the request to
/// the correct handler in a single `use_effect`.
pub fn request_command(kind: crate::app_state::commands::CommandId) {
    *APP_COMMAND_REQUEST_KIND.write() = kind.0;
    let mut counter = APP_COMMAND_REQUEST.write();
    *counter = counter.wrapping_add(1);
}
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

pub fn set_show_bottom_panel(visible: bool) {
    update_ui_settings(|current| {
        current.show_bottom_panel = visible;
    });
}

pub fn set_bottom_panel_height(height: f64) {
    update_ui_settings(|current| {
        current.bottom_panel_height = height;
    });
}

pub fn set_split_mode(mode: WorkspaceSplitMode) {
    update_ui_settings(|current| {
        current.split_mode = mode;
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

/// Dioxus 0.7 writes notify subscribers even when the value is unchanged.
/// Since every `set_*` toggle funnels through `update_ui_settings`, writing
/// every mirror signal here would re-render the whole `.app` subtree on a
/// single panel toggle. Guarding equality scopes re-renders to changed panels.
fn sync_bool(signal: &GlobalSignal<bool>, new: bool) {
    if *signal.peek() != new {
        *signal.write() = new;
    }
}

fn sync_density(signal: &GlobalSignal<UiDensity>, new: UiDensity) {
    if *signal.peek() != new {
        *signal.write() = new;
    }
}

fn sync_f64(signal: &GlobalSignal<f64>, new: f64) {
    if *signal.peek() != new {
        *signal.write() = new;
    }
}

fn sync_split_mode(signal: &GlobalSignal<WorkspaceSplitMode>, new: WorkspaceSplitMode) {
    if *signal.peek() != new {
        *signal.write() = new;
    }
}

fn sync_runtime_ui_settings(settings: &AppUiSettings) {
    let theme_class = settings.theme.css_class().to_string();
    if *APP_THEME.peek() != theme_class {
        *APP_THEME.write() = theme_class;
    }
    sync_density(&APP_UI_DENSITY, settings.density);
    sync_bool(&APP_AI_FEATURES_ENABLED, settings.ai_features_enabled);
    sync_bool(
        &APP_AI_AUTO_APPLY_COMPLETIONS,
        settings.ai_auto_apply_completions,
    );
    sync_bool(&APP_READ_ONLY_MODE, settings.read_only_mode);
    sync_bool(&APP_SHOW_SAVED_QUERIES, settings.show_saved_queries);
    sync_bool(&APP_SHOW_CONNECTIONS, settings.show_connections);
    sync_bool(&APP_SHOW_EXPLORER, settings.show_explorer);
    sync_bool(&APP_SHOW_HISTORY, settings.show_history);
    sync_bool(&APP_SHOW_SQL_EDITOR, settings.show_sql_editor);
    sync_bool(
        &APP_SHOW_AGENT_PANEL,
        settings.ai_features_enabled && settings.show_agent_panel,
    );
    sync_bool(&APP_SHOW_BOTTOM_PANEL, settings.show_bottom_panel);
    sync_f64(&APP_BOTTOM_PANEL_HEIGHT, settings.bottom_panel_height);
    sync_split_mode(&APP_SPLIT_MODE, settings.split_mode);
    crate::screens::workspace::components::agent_panel::prompt::sync_ai_response_language(
        settings.ai_response_language.clone(),
    );
}

/// Записывает сводку последнего запроса в глобальный сигнал статус-бара.
/// `None` сбрасывает запись (например, при отсутствии выполненных запросов).
pub fn set_last_query(summary: Option<LastQuerySummary>) {
    *APP_LAST_QUERY.write() = summary;
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
    // Отравленная блокировка не должна ронять приложение — просто
    // пропускаем регистрацию токена, авто-скрытие тоста по таймеру
    // всё равно сработает. Симметрично с обработкой в `dismiss_toast`.
    if let Ok(mut tokens) = TOAST_CANCEL_TOKENS.lock() {
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

pub fn toast_success(message: impl Into<String>) {
    show_toast(message, ToastKind::Success);
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
    tab_drafts: Vec<models::TabDraft>,
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

    *APP_TAB_DRAFTS.write() = tab_drafts;
    persist_session_state();
}

fn persist_session_state() {
    let (open_requests, active_connection_name, tab_drafts) = {
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
        let drafts = APP_TAB_DRAFTS().clone();
        (requests, active, drafts)
    };

    // Offload synchronous file I/O to a blocking thread so we don't stall the
    // Dioxus render thread.
    spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            services::save_session_state_sync(open_requests, active_connection_name, tab_drafts)
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
    prune_expired(&mut cache, std::time::Instant::now());
    cache.insert(
        session_id,
        ExplorerCacheEntry {
            sections,
            timestamp: std::time::Instant::now(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(timestamp: std::time::Instant) -> ExplorerCacheEntry {
        ExplorerCacheEntry {
            sections: Vec::new(),
            timestamp,
        }
    }

    #[test]
    fn is_expired_at_respects_ttl_boundary() {
        let now = std::time::Instant::now();
        assert!(!entry(now).is_expired_at(now));
        let within_ttl = now - (EXPLORER_CACHE_TTL - Duration::from_millis(1));
        assert!(!entry(within_ttl).is_expired_at(now));
        let just_past = now - (EXPLORER_CACHE_TTL + Duration::from_millis(1));
        assert!(entry(just_past).is_expired_at(now));
    }

    #[test]
    fn prune_expired_removes_only_stale_entries() {
        let now = std::time::Instant::now();
        let mut cache = HashMap::from([
            (
                1u64,
                entry(now - (EXPLORER_CACHE_TTL + Duration::from_secs(1))),
            ),
            (2u64, entry(now - Duration::from_secs(1))),
            (3u64, entry(now)),
        ]);
        assert_eq!(prune_expired(&mut cache, now), 1);
        assert_eq!(cache.len(), 2);
        assert!(cache.contains_key(&2));
        assert!(cache.contains_key(&3));
    }

    #[test]
    fn prune_expired_evicts_everything_when_all_stale() {
        let now = std::time::Instant::now();
        let mut cache = HashMap::from([
            (
                1u64,
                entry(now - (EXPLORER_CACHE_TTL + Duration::from_secs(10))),
            ),
            (
                2u64,
                entry(now - (EXPLORER_CACHE_TTL + Duration::from_secs(5))),
            ),
        ]);
        assert_eq!(prune_expired(&mut cache, now), 2);
        assert!(cache.is_empty());
    }

    fn test_tab(id: u64, title: &str) -> models::QueryTabState {
        let mut tab = models::QueryTabState::default();
        tab.id = id;
        tab.title = title.to_string();
        tab
    }

    #[test]
    fn push_into_strips_pinned_flag() {
        let mut stack: Vec<models::QueryTabState> = Vec::new();
        let mut tab = test_tab(7, "Q");
        tab.pinned = true;
        push_recently_closed_tab_into(&mut stack, tab);
        assert_eq!(stack.len(), 1);
        assert!(!stack[0].pinned, "push must strip the pinned flag");
    }

    #[test]
    fn push_into_prepends_newest_first() {
        let mut stack: Vec<models::QueryTabState> = Vec::new();
        for i in 0..5 {
            push_recently_closed_tab_into(&mut stack, test_tab(i, &format!("Q{i}")));
        }
        let ids: Vec<u64> = stack.iter().map(|t| t.id).collect();
        // Newest first — last pushed at index 0.
        assert_eq!(ids, vec![4, 3, 2, 1, 0]);
    }

    #[test]
    fn push_into_caps_at_limit() {
        let mut stack: Vec<models::QueryTabState> = Vec::new();
        let total = RECENTLY_CLOSED_TABS_LIMIT + 3;
        for i in 0..total {
            push_recently_closed_tab_into(&mut stack, test_tab(i as u64, &format!("Q{i}")));
        }
        assert_eq!(stack.len(), RECENTLY_CLOSED_TABS_LIMIT);
        // The oldest three entries should have been dropped.
        let ids: Vec<u64> = stack.iter().map(|t| t.id).collect();
        assert_eq!(ids[0], (total - 1) as u64);
        assert_eq!(ids[RECENTLY_CLOSED_TABS_LIMIT - 1], 3);
    }

    #[test]
    fn push_into_preserves_tab_content() {
        let mut stack: Vec<models::QueryTabState> = Vec::new();
        let mut tab = test_tab(11, "Q11");
        tab.session_id = 42;
        tab.sql = "select 1".to_string();
        push_recently_closed_tab_into(&mut stack, tab);
        assert_eq!(stack[0].id, 11);
        assert_eq!(stack[0].title, "Q11");
        assert_eq!(stack[0].session_id, 42);
        assert_eq!(stack[0].sql, "select 1");
    }
}
