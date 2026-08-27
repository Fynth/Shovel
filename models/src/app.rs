use crate::{Capabilities, ConnectionRequest, DatabaseKind};

#[derive(Clone, Debug)]
pub struct ConnectionSession {
    pub id: u64,
    pub name: String,
    pub kind: DatabaseKind,
    pub request: ConnectionRequest,
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub sessions: Vec<ConnectionSession>,
    pub active_session_id: Option<u64>,
    pub next_session_id: u64,
    pub show_connection_screen: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session_id: None,
            next_session_id: 1,
            show_connection_screen: true,
        }
    }
}

impl AppState {
    pub fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn active_session(&self) -> Option<&ConnectionSession> {
        let active_id = self.active_session_id?;
        self.session(active_id)
    }

    pub fn session(&self, session_id: u64) -> Option<&ConnectionSession> {
        self.sessions
            .iter()
            .find(|session| session.id == session_id)
    }

    pub fn session_name(&self, session_id: u64) -> Option<String> {
        self.session(session_id).map(|session| session.name.clone())
    }

    pub fn session_id_by_name(&self, name: &str) -> Option<u64> {
        self.sessions
            .iter()
            .find(|session| session.name == name)
            .map(|session| session.id)
    }
}
