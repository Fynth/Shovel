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
