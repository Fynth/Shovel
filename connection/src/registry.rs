use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

use database::SessionHandle;

static REGISTRY: LazyLock<RwLock<HashMap<u64, SessionHandle>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn register_session(id: u64, handle: SessionHandle) {
    REGISTRY
        .write()
        .unwrap_or_else(|err| err.into_inner())
        .insert(id, handle);
}

pub fn unregister_session(id: u64) -> Option<SessionHandle> {
    REGISTRY
        .write()
        .unwrap_or_else(|err| err.into_inner())
        .remove(&id)
}

pub fn session(id: u64) -> Option<SessionHandle> {
    REGISTRY
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .get(&id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{register_session, session, unregister_session};
    use database::{FakeDriver, SessionHandle};
    use std::sync::Arc;

    #[test]
    fn register_get_unregister() {
        let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
        register_session(42, handle.clone());
        assert!(session(42).is_some());
        assert!(unregister_session(42).is_some());
        assert!(session(42).is_none());
    }
}
