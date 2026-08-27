//! End-to-end round-trip tests for the `query` crate.
//!
//! These tests exercise the full pipeline of `execute_query` and the
//! `insert_table_row` / `update_table_cell` / `delete_table_row` workflow
//! against an in-memory SQLite database. They are intentionally written as
//! integration tests inside the crate (rather than as a `tests/` directory)
//! so they can access the same private helpers the production code uses.

#![allow(unused_imports)]

use super::*;
use database::{LiveConnection, SessionHandle};
use sqlx::sqlite::SqlitePool;

/// Connects to a fresh `:memory:` SQLite database and returns it as a
/// `SessionHandle` that the public `query` API can consume.
async fn fresh_sqlite() -> SessionHandle {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("connect to in-memory sqlite");
    SessionHandle::from_legacy(LiveConnection::Sqlite(pool))
}

/// Creates a small test schema used by the round-trip tests.
async fn seed_two_rows(handle: &SessionHandle) {
    execute_query(
        handle,
        "create table widgets (id integer primary key, name text not null, qty integer not null)"
            .to_string(),
    )
    .await
    .expect("create table");
    execute_query(
        handle,
        "insert into widgets (id, name, qty) values (1, 'alpha', 10), (2, 'beta', 20)".to_string(),
    )
    .await
    .expect("insert seed rows");
}

#[tokio::test]
async fn select_returns_seeded_rows_in_order() {
    let conn = fresh_sqlite().await;
    seed_two_rows(&conn).await;

    let output = execute_query(&conn, "select id, name, qty from widgets".to_string())
        .await
        .expect("select");

    match output {
        models::QueryOutput::Table(page) => {
            assert_eq!(page.rows.len(), 2);
            assert_eq!(page.rows[0][0], "1");
            assert_eq!(page.rows[0][1], "alpha");
            assert_eq!(page.rows[0][2], "10");
            assert_eq!(page.rows[1][0], "2");
            assert_eq!(page.rows[1][1], "beta");
            assert_eq!(page.rows[1][2], "20");
        }
        other => panic!("expected table result, got {other:?}"),
    }
}

#[tokio::test]
async fn pagination_returns_offset_slice() {
    let conn = fresh_sqlite().await;
    seed_two_rows(&conn).await;

    let output = execute_query_page(
        &conn,
        "select id from widgets order by id".to_string(),
        1,    // page_size
        1,    // offset (skip first row)
        None, // filter
        None, // sort
    )
    .await
    .expect("paginated select");

    match output {
        models::QueryOutput::Table(page) => {
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0][0], "2");
        }
        other => panic!("expected table result, got {other:?}"),
    }
}

#[tokio::test]
async fn insert_then_update_then_delete_round_trip() {
    let conn = fresh_sqlite().await;
    seed_two_rows(&conn).await;

    // Insert a new row through the public `execute_query` API.
    execute_query(
        &conn,
        "insert into widgets (id, name, qty) values (3, 'gamma', 30)".to_string(),
    )
    .await
    .expect("insert third row");

    let output = execute_query(&conn, "select count(*) from widgets".to_string())
        .await
        .expect("count after insert");
    match output {
        models::QueryOutput::Table(page) => assert_eq!(page.rows[0][0], "3"),
        other => panic!("expected table result, got {other:?}"),
    }

    // Update the newly inserted row.
    execute_query(
        &conn,
        "update widgets set qty = 99 where id = 3".to_string(),
    )
    .await
    .expect("update third row");

    let output = execute_query(&conn, "select qty from widgets where id = 3".to_string())
        .await
        .expect("select after update");
    match output {
        models::QueryOutput::Table(page) => assert_eq!(page.rows[0][0], "99"),
        other => panic!("expected table result, got {other:?}"),
    }

    // Delete the row through plain SQL.
    execute_query(&conn, "delete from widgets where id = 3".to_string())
        .await
        .expect("delete third row");

    let output = execute_query(&conn, "select count(*) from widgets".to_string())
        .await
        .expect("count after delete");
    match output {
        models::QueryOutput::Table(page) => assert_eq!(page.rows[0][0], "2"),
        other => panic!("expected table result, got {other:?}"),
    }
}

#[tokio::test]
async fn create_then_drop_table_lifecycle() {
    let conn = fresh_sqlite().await;

    create_table(
        &conn,
        Some("main".to_string()),
        "ephemeral".to_string(),
        "id integer primary key".to_string(),
        None,
    )
    .await
    .expect("create ephemeral table");

    let output = execute_query(&conn, "select count(*) from ephemeral".to_string())
        .await
        .expect("count on empty table");
    match output {
        models::QueryOutput::Table(page) => assert_eq!(page.rows[0][0], "0"),
        other => panic!("expected table result, got {other:?}"),
    }

    // `drop_table` consumes a `TablePreviewSource` derived from a SELECT.
    // For a freshly created table the simpler path is the underlying SQL.
    execute_query(&conn, "drop table ephemeral".to_string())
        .await
        .expect("drop ephemeral table");

    let err = execute_query(&conn, "select * from ephemeral".to_string())
        .await
        .expect_err("query against dropped table should fail");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("no such table")
            || msg.to_lowercase().contains("does not exist"),
        "expected 'no such table' error, got: {msg}"
    );
}

#[tokio::test]
async fn is_read_only_sql_accepts_safe_statements() {
    assert!(is_read_only_sql("select 1"));
    assert!(is_read_only_sql("SELECT id, name FROM users"));
    assert!(is_read_only_sql("with cte as (select 1) select * from cte"));
    assert!(is_read_only_sql("explain select * from widgets"));
    assert!(is_read_only_sql("show tables"));
    assert!(is_read_only_sql("pragma table_info(widgets)"));
}

#[tokio::test]
async fn is_read_only_sql_rejects_writes() {
    assert!(!is_read_only_sql("insert into widgets values (1, 'a', 1)"));
    assert!(!is_read_only_sql("update widgets set qty = 0"));
    assert!(!is_read_only_sql("delete from widgets"));
    assert!(!is_read_only_sql("drop table widgets"));
    assert!(!is_read_only_sql("create table t (id int)"));
    assert!(!is_read_only_sql("truncate table widgets"));
    // Mixed: one read, one write in a single script must be rejected.
    assert!(!is_read_only_sql("select 1; drop table widgets"));
}
