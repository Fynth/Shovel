use models::ClickHouseFormData;
use std::{sync::OnceLock, time::Duration};

// Re-export ClickHouse JSON types from models for backward compatibility.
pub use models::{ClickHouseJsonMeta, ClickHouseJsonResponse};

pub struct ClickHouseDriver;

impl database::DatabaseDriver for ClickHouseDriver {
    type Config = ClickHouseFormData;
    type Pool = ClickHouseFormData;
    type Error = String;

    async fn connect(info: Self::Config) -> Result<Self::Pool, Self::Error> {
        execute_text_query(&info, "SELECT 1").await?;
        Ok(info)
    }

    async fn execute_json_query(
        &self,
        config: &ClickHouseFormData,
        sql: &str,
    ) -> Result<ClickHouseJsonResponse, models::DatabaseError> {
        execute_json_query(config, sql)
            .await
            .map_err(models::DatabaseError::ClickHouse)
    }

    async fn execute_text_query(
        &self,
        config: &ClickHouseFormData,
        sql: &str,
    ) -> Result<String, models::DatabaseError> {
        execute_text_query(config, sql)
            .await
            .map_err(models::DatabaseError::ClickHouse)
    }
}

pub async fn execute_json_query(
    info: &ClickHouseFormData,
    sql: &str,
) -> Result<ClickHouseJsonResponse, String> {
    let sql = ensure_json_compact(sql);
    let response = build_request(info, &sql)?
        .send()
        .await
        .map_err(|err| format!("ClickHouse request failed: {err}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read ClickHouse response: {err}"))?;

    if !status.is_success() {
        return Err(format!("ClickHouse returned {status}: {body}"));
    }

    serde_json::from_str(&body).map_err(|err| format!("Failed to parse ClickHouse JSON: {err}"))
}

pub async fn execute_text_query(info: &ClickHouseFormData, sql: &str) -> Result<String, String> {
    let response = build_request(info, sql)?
        .send()
        .await
        .map_err(|err| format!("ClickHouse request failed: {err}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read ClickHouse response: {err}"))?;

    if !status.is_success() {
        return Err(format!("ClickHouse returned {status}: {body}"));
    }

    Ok(body)
}

fn build_request(info: &ClickHouseFormData, sql: &str) -> Result<reqwest::RequestBuilder, String> {
    let base_url = normalize_base_url(info)?;
    let client = http_client()?;
    let mut request = client.post(format!("{base_url}/")).body(sql.to_string());

    request = request.basic_auth(info.effective_username(), Some(&info.password));
    request = request.query(&[("database", info.effective_database())]);

    Ok(request)
}

fn ensure_json_compact(sql: &str) -> String {
    let trimmed = sql
        .trim()
        .trim_end_matches(|c: char| c == ';' || c.is_whitespace());
    if trimmed.to_ascii_lowercase().contains(" format ") {
        trimmed.to_string()
    } else {
        format!("{trimmed} FORMAT JSONCompact")
    }
}

fn normalize_base_url(info: &ClickHouseFormData) -> Result<String, String> {
    let host = info.host.trim();
    if host.is_empty() {
        return Err("ClickHouse host is empty".to_string());
    }

    let normalized = if host.starts_with("http://") || host.starts_with("https://") {
        host.trim_end_matches('/').to_string()
    } else {
        format!("http://{}:{}", host, info.port)
    };

    Ok(normalized)
}

fn http_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|err| format!("failed to build ClickHouse HTTP client: {err}"))
    }) {
        Ok(client) => Ok(client),
        Err(err) => Err(err.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_base_url tests ────────────────────────────────────

    #[test]
    fn normalize_base_url_with_http_scheme() {
        let info = ClickHouseFormData {
            host: "http://clickhouse.example.com".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "http://clickhouse.example.com"
        );
    }

    #[test]
    fn normalize_base_url_with_https_scheme() {
        let info = ClickHouseFormData {
            host: "https://clickhouse.example.com".into(),
            port: 8443,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "https://clickhouse.example.com"
        );
    }

    #[test]
    fn normalize_base_url_without_scheme_appends_default_port() {
        let info = ClickHouseFormData {
            host: "clickhouse.example.com".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "http://clickhouse.example.com:8123"
        );
    }

    #[test]
    fn normalize_base_url_without_scheme_custom_port() {
        let info = ClickHouseFormData {
            host: "ch.internal".into(),
            port: 8443,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "http://ch.internal:8443"
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_slash_from_http() {
        let info = ClickHouseFormData {
            host: "http://clickhouse.example.com/".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "http://clickhouse.example.com"
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_slashes() {
        let info = ClickHouseFormData {
            host: "https://clickhouse.example.com///".into(),
            port: 8443,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(
            normalize_base_url(&info).unwrap(),
            "https://clickhouse.example.com"
        );
    }

    #[test]
    fn normalize_base_url_ip_address_without_scheme() {
        let info = ClickHouseFormData {
            host: "10.0.0.5".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(normalize_base_url(&info).unwrap(), "http://10.0.0.5:8123");
    }

    #[test]
    fn normalize_base_url_ip_address_with_scheme() {
        let info = ClickHouseFormData {
            host: "http://10.0.0.5".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(normalize_base_url(&info).unwrap(), "http://10.0.0.5");
    }

    #[test]
    fn normalize_base_url_localhost() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(normalize_base_url(&info).unwrap(), "http://localhost:8123");
    }

    #[test]
    fn normalize_base_url_empty_host_returns_error() {
        let info = ClickHouseFormData {
            host: String::new(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert!(normalize_base_url(&info).is_err());
        assert_eq!(
            normalize_base_url(&info).unwrap_err(),
            "ClickHouse host is empty"
        );
    }

    #[test]
    fn normalize_base_url_whitespace_host_returns_error() {
        let info = ClickHouseFormData {
            host: "   ".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert!(normalize_base_url(&info).is_err());
        assert_eq!(
            normalize_base_url(&info).unwrap_err(),
            "ClickHouse host is empty"
        );
    }

    // ── ensure_json_compact tests ───────────────────────────────────

    #[test]
    fn ensure_json_compact_appends_format_to_plain_select() {
        let sql = "SELECT 1";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT 1 FORMAT JSONCompact");
    }

    #[test]
    fn ensure_json_compact_appends_format_to_multiline_sql() {
        let sql = "SELECT\n  id,\n  name\nFROM users\nWHERE active = 1";
        let result = ensure_json_compact(sql);
        assert_eq!(
            result,
            "SELECT\n  id,\n  name\nFROM users\nWHERE active = 1 FORMAT JSONCompact"
        );
    }

    #[test]
    fn ensure_json_compact_preserves_existing_format_clause() {
        let sql = "SELECT * FROM events FORMAT TSV";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT * FROM events FORMAT TSV");
    }

    #[test]
    fn ensure_json_compact_handles_lowercase_format() {
        let sql = "select 1 format csv";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "select 1 format csv");
    }

    #[test]
    fn ensure_json_compact_handles_mixed_case_format() {
        let sql = "SELECT 1 FORMAT JsonCompact";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT 1 FORMAT JsonCompact");
    }

    #[test]
    fn ensure_json_compact_strips_trailing_semicolon() {
        let sql = "SELECT 1;";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT 1 FORMAT JSONCompact");
    }

    #[test]
    fn ensure_json_compact_strips_multiple_semicolons_and_spaces() {
        let sql = "SELECT 1 ; ;  ";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT 1 FORMAT JSONCompact");
    }

    #[test]
    fn ensure_json_compact_handles_format_with_trailing_semicolon() {
        let sql = "SELECT * FROM t FORMAT CSV;";
        let result = ensure_json_compact(sql);
        assert_eq!(result, "SELECT * FROM t FORMAT CSV");
    }

    #[test]
    fn ensure_json_compact_empty_string() {
        let sql = "";
        let result = ensure_json_compact(sql);
        assert_eq!(result, " FORMAT JSONCompact");
    }

    #[test]
    fn ensure_json_compact_whitespace_only() {
        let sql = "   ";
        let result = ensure_json_compact(sql);
        assert_eq!(result, " FORMAT JSONCompact");
    }

    // ── ClickHouseFormData effective_username / effective_database ──

    #[test]
    fn effective_username_default_when_empty() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: String::new(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_username(), "default");
    }

    #[test]
    fn effective_username_default_when_whitespace() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "   ".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_username(), "default");
    }

    #[test]
    fn effective_username_returns_provided_value() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "admin".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_username(), "admin");
    }

    #[test]
    fn effective_username_trims_whitespace() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "  read_only  ".into(),
            password: String::new(),
            database: "default".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_username(), "read_only");
    }

    #[test]
    fn effective_database_default_when_empty() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: String::new(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_database(), "default");
    }

    #[test]
    fn effective_database_default_when_whitespace() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "   ".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_database(), "default");
    }

    #[test]
    fn effective_database_returns_provided_value() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "analytics".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_database(), "analytics");
    }

    #[test]
    fn effective_database_trims_whitespace() {
        let info = ClickHouseFormData {
            host: "localhost".into(),
            port: 8123,
            username: "default".into(),
            password: String::new(),
            database: "  system  ".into(),
            ssh_tunnel: None,
        };
        assert_eq!(info.effective_database(), "system");
    }

    // ── ClickHouseJsonMeta deserialization ──────────────────────────

    #[test]
    fn clickhouse_json_meta_deserializes_basic() {
        let json = r#"{"name":"id","type":"UInt32"}"#;
        let meta: ClickHouseJsonMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "id");
        assert_eq!(meta.data_type, "UInt32");
    }

    #[test]
    fn clickhouse_json_meta_deserializes_nullable_type() {
        let json = r#"{"name":"email","type":"Nullable(String)"}"#;
        let meta: ClickHouseJsonMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "email");
        assert_eq!(meta.data_type, "Nullable(String)");
    }

    #[test]
    fn clickhouse_json_meta_deserializes_low_cardinality_type() {
        let json = r#"{"name":"status","type":"LowCardinality(String)"}"#;
        let meta: ClickHouseJsonMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "status");
        assert_eq!(meta.data_type, "LowCardinality(String)");
    }

    #[test]
    fn clickhouse_json_meta_deserializes_array_type() {
        let json = r#"{"name":"tags","type":"Array(String)"}"#;
        let meta: ClickHouseJsonMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "tags");
        assert_eq!(meta.data_type, "Array(String)");
    }

    #[test]
    fn clickhouse_json_meta_deserializes_decimal_type() {
        let json = r#"{"name":"price","type":"Decimal(10,2)"}"#;
        let meta: ClickHouseJsonMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.name, "price");
        assert_eq!(meta.data_type, "Decimal(10,2)");
    }

    // ── ClickHouseJsonResponse deserialization ──────────────────────

    #[test]
    fn clickhouse_json_response_deserializes_empty() {
        let json = r#"{"meta":[],"data":[]}"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.is_empty());
        assert!(resp.data.is_empty());
    }

    #[test]
    fn clickhouse_json_response_deserializes_with_rows() {
        let json = r#"{
                "meta":[
                    {"name":"id","type":"UInt32"},
                    {"name":"name","type":"String"}
                ],
                "data":[
                    [1,"Alice"],
                    [2,"Bob"]
                ]
            }"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.meta.len(), 2);
        assert_eq!(resp.meta[0].name, "id");
        assert_eq!(resp.meta[1].name, "name");
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0][0], serde_json::json!(1));
        assert_eq!(resp.data[0][1], serde_json::json!("Alice"));
        assert_eq!(resp.data[1][0], serde_json::json!(2));
        assert_eq!(resp.data[1][1], serde_json::json!("Bob"));
    }

    #[test]
    fn clickhouse_json_response_deserializes_null_values() {
        let json = r#"{
                "meta":[{"name":"val","type":"Nullable(String)"}],
                "data":[["hello"],[null]]
            }"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data[0][0], serde_json::json!("hello"));
        assert_eq!(resp.data[1][0], serde_json::json!(null));
    }

    #[test]
    fn clickhouse_json_response_deserializes_various_types() {
        let json = r#"{
                "meta":[
                    {"name":"int_col","type":"Int32"},
                    {"name":"float_col","type":"Float64"},
                    {"name":"str_col","type":"String"},
                    {"name":"arr_col","type":"Array(Int32)"}
                ],
                "data":[[42,2.71,"test",[1,2,3]]]
            }"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        let row = &resp.data[0];
        assert_eq!(row[0], serde_json::json!(42));
        assert_eq!(row[1], serde_json::json!(2.71));
        assert_eq!(row[2], serde_json::json!("test"));
        assert_eq!(row[3], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn clickhouse_json_response_deserializes_without_meta_field() {
        // meta is #[serde(default)] — should handle missing gracefully
        let json = r#"{"data":[[1]]}"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        assert!(resp.meta.is_empty());
        assert_eq!(resp.data.len(), 1);
    }

    #[test]
    fn clickhouse_json_response_deserializes_without_data_field() {
        // data is #[serde(default)] — should handle missing gracefully
        let json = r#"{"meta":[{"name":"x","type":"Int32"}]}"#;
        let resp: ClickHouseJsonResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.meta.len(), 1);
        assert!(resp.data.is_empty());
    }

    // ── Debug and Display trait checks ──────────────────────────────

    #[test]
    fn clickhouse_json_meta_debug_output() {
        let meta = ClickHouseJsonMeta {
            name: "id".into(),
            data_type: "UInt32".into(),
        };
        let debug = format!("{:?}", meta);
        assert!(debug.contains("id"));
        assert!(debug.contains("UInt32"));
    }

    #[test]
    fn clickhouse_json_response_debug_output() {
        let resp = ClickHouseJsonResponse {
            meta: vec![ClickHouseJsonMeta {
                name: "col".into(),
                data_type: "String".into(),
            }],
            data: vec![vec![serde_json::json!("val")]],
        };
        let debug = format!("{:?}", resp);
        assert!(debug.contains("col"));
        assert!(debug.contains("val"));
    }

    // ── ClickHouseDriver constants / type checks ────────────────────

    #[test]
    fn clickhouse_driver_associated_types() {
        // Compile-time check: ClickHouseDriver uses ClickHouseFormData
        // for both Config and Pool, and String for Error.
        fn _assert_types() {
            // These are compile-time assertions — if they compile, they pass.
            let _: <ClickHouseDriver as database::DatabaseDriver>::Config = ClickHouseFormData {
                host: "localhost".into(),
                port: 8123,
                username: "default".into(),
                password: String::new(),
                database: "default".into(),
                ssh_tunnel: None,
            };
            let _: <ClickHouseDriver as database::DatabaseDriver>::Error = String::new();
        }
        _assert_types()
    }
}
