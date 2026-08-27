//! Dev-only utilities for working without a real database.
//!
//! Everything in this module is gated behind `debug_assertions`, so
//! release builds cannot accidentally expose a mock-data backdoor. The
//! public surface is intentionally tiny:
//!
//! - [`MockDatabaseRepository`] returns hand-crafted fake explorer
//!   trees and previews (see `mock_repository`).
//! - [`install_mock_explorer`] wires a real in-memory SQLite session
//!   into `APP_STATE` and overlays the empty real tree with the mock
//!   one in the explorer cache, so the rest of the workspace renders
//!   fake data without any DB connection.
//! - [`is_mock_session`] / [`mock_preview_for`] are the small hooks
//!   the workspace actions consult before delegating to the real
//!   `services::execute_query*` path.

mod mock_repository;

pub use mock_repository::{
    MOCK_CONNECTION_DISPLAY_NAME,
    MOCK_CONNECTION_IDENTITY_KEY,
    MockDatabaseRepository,
};

use models::{ConnectionRequest, DatabaseKind, QueryOutput, SqliteFormData, TablePreviewSource};

use crate::{app_state::APP_STATE, screens::workspace::ExplorerConnectionSection};
use dioxus::prelude::ReadableExt;

/// Pure name predicate. Extracted so unit tests do not need a Dioxus
/// runtime.
pub fn is_mock_session_name(name: &str) -> bool {
    name == MOCK_CONNECTION_DISPLAY_NAME
}

/// Returns `true` when the given session id belongs to the mock
/// session. The mock session is identified by its display name
/// (`Mock (dev)`) rather than a fixed id, so the answer is derived
/// from the live `APP_STATE` snapshot.
pub fn is_mock_session(session_id: u64) -> bool {
    APP_STATE
        .read()
        .session(session_id)
        .is_some_and(|session| is_mock_session_name(&session.name))
}

/// Returns a `QueryOutput` from the mock repo if the source names a
/// known mock table, otherwise `None`. The workspace's
/// `run_table_preview_for_tab` calls this before delegating to the
/// real DB so the user sees a useful grid even though the connection
/// is an empty `:memory:` pool.
pub fn mock_preview_for(source: &TablePreviewSource) -> Option<QueryOutput> {
    MockDatabaseRepository::new().preview_for(source)
}

/// Returns a `QueryOutput` for an ad-hoc SQL string when the
/// statement is a `select * from <known_table>` against one of the
/// mock tables. Returns `None` for anything else so the workspace
/// can fall through to the real `services::execute_query_page`.
pub fn mock_query_for(sql: &str) -> Option<QueryOutput> {
    MockDatabaseRepository::new().query_for(sql)
}

/// Build the `ConnectionRequest` that the workspace sends to
/// `services::connect_and_save_request` when activating mock mode.
/// The path `:memory:` is routed by `driver-sqlite` to a fresh
/// in-memory pool, so the real connection layer still works.
pub fn mock_connection_request() -> ConnectionRequest {
    ConnectionRequest::Sqlite(SqliteFormData {
        path: ":memory:".to_string(),
    })
}

/// Connect a fresh in-memory SQLite session and overlay the empty
/// real tree with hand-crafted mock nodes. The session is added to
/// [`crate::app_state::APP_STATE`] so the rest of the workspace
/// (status bar, query tabs, ACP panel) can reference it by id; the
/// explorer cache is seeded with the mock tree so the user sees the
/// fake data immediately.
///
/// Idempotent: a second call is a no-op (the existing mock session is
/// kept and the cache is refreshed). Returns the session id.
pub async fn install_mock_explorer() -> u64 {
    if let Some(existing) = mock_session_id() {
        seed_explorer_cache(existing).await;
        return existing;
    }

    let request = mock_connection_request();
    let kind = request.kind();
    let handle = match services::connect_to_db(request.clone()).await {
        Ok(handle) => handle,
        Err(err) => {
            crate::app_state::toast_error(format!("Mock connection failed: {err}"));
            return 0;
        }
    };
    let session_id = crate::app_state::add_connection_session(request, handle);
    debug_assert_eq!(kind, DatabaseKind::Sqlite);
    seed_explorer_cache(session_id).await;
    session_id
}

/// Look up the mock session's id in `APP_STATE` without going through
/// a real connection. Returns `None` if the mock has not been
/// installed.
pub fn mock_session_id() -> Option<u64> {
    APP_STATE
        .read()
        .session_id_by_name(MOCK_CONNECTION_DISPLAY_NAME)
}

/// Build the [`ExplorerConnectionSection`]s for the mock session
/// without writing them to the cache. Useful in tests and in the dev
/// toggle's preview panel.
pub fn mock_sections(session_id: u64) -> Vec<ExplorerConnectionSection> {
    MockDatabaseRepository::new().tree_sections(session_id)
}

async fn seed_explorer_cache(session_id: u64) {
    let sections = MockDatabaseRepository::new().tree_sections(session_id);
    crate::app_state::cache_explorer(session_id, sections).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_mock_session_name_recognises_mock_display_name() {
        assert!(is_mock_session_name(MOCK_CONNECTION_DISPLAY_NAME));
        assert!(!is_mock_session_name("local-postgres"));
        assert!(!is_mock_session_name(""));
    }

    #[test]
    fn mock_connection_request_is_sqlite_memory() {
        let req = mock_connection_request();
        assert_eq!(req.kind(), DatabaseKind::Sqlite);
        assert_eq!(req.identity_key(), MOCK_CONNECTION_IDENTITY_KEY);
    }

    #[test]
    fn mock_sections_use_supplied_session_id() {
        let sections = mock_sections(1234);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].session_id, 1234);
        assert!(sections[0].is_active);
    }
}
