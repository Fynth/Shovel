//! Integration tests for the `models` crate.
//!
//! These tests exercise the public API surface that the rest of the workspace
//! depends on, including:
//!
//! - `DatabaseConnection` / `ConnectionRequest` serde roundtrips
//! - `DatabaseError` display + kinds
//! - `DatabaseKind` <-> `ConnectionRequest` conversions
//! - Form-data construction
//!
//! They are kept separate from the in-module unit tests so that any compile
//! failure in a public type is caught here, even if the unit tests inside
//! the module are filtered out.

use models::{
    ClickHouseFormData,
    ConnectionRequest,
    DatabaseError,
    DatabaseKind,
    MySqlFormData,
    PostgresFormData,
    SavedConnection,
    SqliteFormData,
    SshTunnelConfig,
};

fn sqlite_request() -> ConnectionRequest {
    ConnectionRequest::Sqlite(SqliteFormData {
        path: "/tmp/example.db".to_string(),
    })
}

fn postgres_request() -> ConnectionRequest {
    ConnectionRequest::Postgres(PostgresFormData {
        host: "localhost".to_string(),
        port: 5432,
        username: "postgres".to_string(),
        password: "secret".to_string(),
        database: "app".to_string(),
        ssl_mode: "prefer".to_string(),
        ssh_tunnel: None,
    })
}

fn mysql_request() -> ConnectionRequest {
    ConnectionRequest::MySql(MySqlFormData {
        host: "localhost".to_string(),
        port: 3306,
        username: "root".to_string(),
        password: "secret".to_string(),
        database: "app".to_string(),
        ssl_mode: "preferred".to_string(),
        ssh_tunnel: None,
    })
}

fn clickhouse_request() -> ConnectionRequest {
    ConnectionRequest::ClickHouse(empty_clickhouse_form())
}

fn empty_clickhouse_form() -> ClickHouseFormData {
    ClickHouseFormData {
        host: "localhost".to_string(),
        port: 8123,
        username: "default".to_string(),
        password: "".to_string(),
        database: "default".to_string(),
        ssh_tunnel: None,
    }
}

#[test]
fn connection_request_roundtrips_through_json() {
    for req in [
        sqlite_request(),
        postgres_request(),
        mysql_request(),
        clickhouse_request(),
    ] {
        let serialized = serde_json::to_string(&req).expect("serialize");
        let parsed: ConnectionRequest = serde_json::from_str(&serialized).expect("parse");
        assert_eq!(parsed, req, "roundtrip mismatch for {req:?}");
    }
}

#[test]
fn all_database_kinds_have_distinct_serde_tags() {
    // The enum is a tuple variant and serde keeps the variant name verbatim.
    let variants = [
        ("Sqlite", sqlite_request()),
        ("Postgres", postgres_request()),
        ("MySql", mysql_request()),
        ("ClickHouse", clickhouse_request()),
    ];
    let mut seen = std::collections::HashSet::new();
    for (label, req) in &variants {
        let json = serde_json::to_value(req).expect("serialize");
        let obj = json.as_object().expect("connection serializes to object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let owned_label = label.to_string();
        assert!(
            keys.contains(&&owned_label),
            "expected `{label}` variant to expose a top-level `{label}` key, got keys: {keys:?}"
        );
        assert!(
            seen.insert(label.to_string()),
            "duplicate variant label `{label}` — all variants must be unique"
        );
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn saved_connection_roundtrips_through_json() {
    let conn = SavedConnection {
        name: "primary".into(),
        request: postgres_request(),
    };
    let json = serde_json::to_string(&conn).expect("serialize");
    let parsed: SavedConnection = serde_json::from_str(&json).expect("parse");
    assert_eq!(parsed.name, "primary");
    assert_eq!(parsed.request, conn.request);
}

#[test]
fn database_error_kind_reports_origin() {
    let ch = DatabaseError::ClickHouse("bad request".into());
    assert_eq!(ch.kind(), Some(DatabaseKind::ClickHouse));
    let tunnel = DatabaseError::Tunnel("ssh down".into());
    assert_eq!(tunnel.kind(), None);
}

#[test]
fn database_error_displays_with_kind_prefix() {
    let err = DatabaseError::ClickHouse("bad request".into());
    let rendered = err.to_string();
    assert!(
        rendered.contains("ClickHouse"),
        "Display impl should mention the driver kind, got: {rendered}"
    );
}

#[test]
fn clickhouse_effective_username_falls_back_to_default() {
    let mut form = empty_clickhouse_form();
    form.username = "   ".to_string();
    assert_eq!(form.effective_username(), "default");
    form.username = "alice".to_string();
    assert_eq!(form.effective_username(), "alice");
}

#[test]
fn clickhouse_effective_database_falls_back_to_default() {
    let mut form = empty_clickhouse_form();
    form.database = "".to_string();
    assert_eq!(form.effective_database(), "default");
    form.database = "  ".to_string();
    assert_eq!(form.effective_database(), "default");
    form.database = "prod".to_string();
    assert_eq!(form.effective_database(), "prod");
}

#[test]
fn ssh_tunnel_config_default_is_empty_and_serializes_to_object() {
    let ssh = SshTunnelConfig::default();
    let json = serde_json::to_value(&ssh).expect("serialize");
    assert!(json.is_object());
}

#[test]
fn request_kind_matches_database_kind() {
    assert_eq!(sqlite_request().kind(), DatabaseKind::Sqlite);
    assert_eq!(postgres_request().kind(), DatabaseKind::Postgres);
    assert_eq!(mysql_request().kind(), DatabaseKind::MySql);
    assert_eq!(clickhouse_request().kind(), DatabaseKind::ClickHouse);
}
