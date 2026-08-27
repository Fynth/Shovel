use database::{FakeDriver, SessionHandle};
use std::sync::Arc;

#[tokio::test]
async fn explorer_proxy_uses_fake_schema() {
    let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
    let tree = explorer::load_connection_tree(&handle).await.unwrap();
    assert!(!tree.is_empty());
}
