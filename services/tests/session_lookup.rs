#[tokio::test]
async fn execute_unknown_session_is_session_not_found() {
    let err = services::execute_query_page(999_999, "select 1".into(), 10, 0, None, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        models::DatabaseError::SessionNotFound(999_999)
    ));
}

#[tokio::test]
async fn execute_after_unregister_is_session_not_found() {
    use database::{FakeDriver, SessionHandle};
    use std::sync::Arc;

    let session_id = 20_260_827;
    let handle = SessionHandle::wrap(Arc::new(FakeDriver::default()));
    services::register_session(session_id, handle);

    let first =
        services::execute_query_page(session_id, "select 1".into(), 10, 0, None, None).await;
    assert!(
        first.is_ok(),
        "expected query to succeed while registered: {first:?}"
    );

    assert!(services::unregister_session(session_id).is_some());

    let err = services::execute_query_page(session_id, "select 1".into(), 10, 0, None, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        models::DatabaseError::SessionNotFound(20_260_827)
    ));
}
