use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, path::Path};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseKind {
    Sqlite,
    Postgres,
    MySql,
    ClickHouse,
}

impl DatabaseKind {
    /// Returns the human-facing display name for this database kind.
    pub fn display_name(&self) -> &'static str {
        match self {
            DatabaseKind::Sqlite => "SQLite",
            DatabaseKind::Postgres => "PostgreSQL",
            DatabaseKind::MySql => "MySQL",
            DatabaseKind::ClickHouse => "ClickHouse",
        }
    }

    /// Returns the default TCP port for this database kind, or `None` for file-based databases.
    pub fn default_port(&self) -> Option<u16> {
        match self {
            DatabaseKind::Sqlite => None,
            DatabaseKind::Postgres => Some(5432),
            DatabaseKind::MySql => Some(3306),
            DatabaseKind::ClickHouse => Some(8123),
        }
    }

    /// Returns `true` if this database kind supports SSH tunnel connections.
    pub fn supports_ssh_tunnel(&self) -> bool {
        !matches!(self, DatabaseKind::Sqlite)
    }

    /// Returns `true` if this database kind supports row-level editing.
    pub fn supports_row_editing(&self) -> bool {
        !matches!(self, DatabaseKind::ClickHouse)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub row_editing: bool,
    pub explain: bool,
    pub transactions: bool,
    pub schemas: bool,
    pub import_csv: bool,
    pub ssh_tunnel: bool,
}

impl Capabilities {
    pub fn for_kind(kind: DatabaseKind) -> Self {
        match kind {
            DatabaseKind::Sqlite => Self {
                row_editing: true,
                explain: true,
                transactions: true,
                schemas: false,
                import_csv: true,
                ssh_tunnel: false,
            },
            DatabaseKind::Postgres | DatabaseKind::MySql => Self {
                row_editing: true,
                explain: true,
                transactions: true,
                schemas: true,
                import_csv: true,
                ssh_tunnel: true,
            },
            DatabaseKind::ClickHouse => Self {
                row_editing: false,
                explain: true,
                transactions: false,
                schemas: true,
                import_csv: true,
                ssh_tunnel: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub private_key_path: String,
}

impl SshTunnelConfig {
    pub fn is_configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.username.trim().is_empty()
    }

    pub fn effective_port(&self) -> u16 {
        if self.port == 0 { 22 } else { self.port }
    }

    pub fn display_name(&self) -> String {
        format!(
            "{}@{}:{}",
            self.username.trim(),
            self.host.trim(),
            self.effective_port()
        )
    }
}

#[derive(Debug)]
pub enum DatabaseError {
    Driver(String),
    Tunnel(String),
    Unsupported(String),
    SessionNotFound(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqliteFormData {
    pub path: String,
}

fn default_postgres_ssl_mode() -> String {
    "prefer".to_string()
}

fn default_mysql_ssl_mode() -> String {
    "preferred".to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostgresFormData {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    #[serde(default)]
    pub ssh_tunnel: Option<SshTunnelConfig>,
    #[serde(default = "default_postgres_ssl_mode")]
    pub ssl_mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MySqlFormData {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    #[serde(default)]
    pub ssh_tunnel: Option<SshTunnelConfig>,
    #[serde(default = "default_mysql_ssl_mode")]
    pub ssl_mode: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClickHouseFormData {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    #[serde(default)]
    pub ssh_tunnel: Option<SshTunnelConfig>,
}

impl ClickHouseFormData {
    pub fn effective_username(&self) -> &str {
        if self.username.trim().is_empty() {
            "default"
        } else {
            self.username.trim()
        }
    }

    pub fn effective_database(&self) -> &str {
        if self.database.trim().is_empty() {
            "default"
        } else {
            self.database.trim()
        }
    }
}

/// Column metadata returned by ClickHouse JSONCompact format.
#[derive(Clone, Debug, Deserialize)]
pub struct ClickHouseJsonMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
}

/// A parsed ClickHouse JSON response.
#[derive(Clone, Debug, Deserialize)]
pub struct ClickHouseJsonResponse {
    #[serde(default)]
    pub meta: Vec<ClickHouseJsonMeta>,
    #[serde(default)]
    pub data: Vec<Vec<serde_json::Value>>,
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) | Self::Unsupported(err) => write!(f, "{err}"),
            Self::Tunnel(err) => write!(f, "SSH tunnel error: {err}"),
            Self::SessionNotFound(id) => write!(f, "session {id} is not connected"),
        }
    }
}

impl Error for DatabaseError {}

impl DatabaseError {
    /// Returns the [`fmt::Display`] rendering of this error.
    pub fn display_string(&self) -> String {
        format!("{self}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionRequest {
    Sqlite(SqliteFormData),
    Postgres(PostgresFormData),
    MySql(MySqlFormData),
    ClickHouse(ClickHouseFormData),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub name: String,
    pub request: ConnectionRequest,
}

impl ConnectionRequest {
    pub fn kind(&self) -> DatabaseKind {
        match self {
            ConnectionRequest::Sqlite(_) => DatabaseKind::Sqlite,
            ConnectionRequest::Postgres(_) => DatabaseKind::Postgres,
            ConnectionRequest::MySql(_) => DatabaseKind::MySql,
            ConnectionRequest::ClickHouse(_) => DatabaseKind::ClickHouse,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            ConnectionRequest::Sqlite(data) => format!("SQLite · {}", data.path.trim()),
            ConnectionRequest::Postgres(data) => {
                let endpoint = normalized_postgres_endpoint(data);
                let mut label = format!(
                    "PostgreSQL · {}@{}:{}/{}",
                    endpoint.username, endpoint.host, endpoint.port, endpoint.database
                );
                if let Some(tunnel) = data.ssh_tunnel.as_ref().filter(|cfg| cfg.is_configured()) {
                    label.push_str(&format!(" via SSH {}", tunnel.display_name()));
                }
                label
            }
            ConnectionRequest::MySql(data) => {
                let endpoint = normalized_mysql_endpoint(data);
                let mut label = format!(
                    "MySQL · {}@{}:{}",
                    endpoint.username, endpoint.host, endpoint.port
                );
                if !endpoint.database.is_empty() {
                    label.push('/');
                    label.push_str(&endpoint.database);
                }
                if let Some(tunnel) = data.ssh_tunnel.as_ref().filter(|cfg| cfg.is_configured()) {
                    label.push_str(&format!(" via SSH {}", tunnel.display_name()));
                }
                label
            }
            ConnectionRequest::ClickHouse(data) => {
                let mut label = format!(
                    "ClickHouse · {}@{}/{}",
                    data.effective_username(),
                    clickhouse_endpoint_label(data),
                    data.effective_database()
                );
                if let Some(tunnel) = data.ssh_tunnel.as_ref().filter(|cfg| cfg.is_configured()) {
                    label.push_str(&format!(" via SSH {}", tunnel.display_name()));
                }
                label
            }
        }
    }

    pub fn short_name(&self) -> String {
        match self {
            ConnectionRequest::Sqlite(data) => Path::new(data.path.trim())
                .file_stem()
                .or_else(|| Path::new(data.path.trim()).file_name())
                .map(|value| value.to_string_lossy().into_owned())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| data.path.trim().to_string()),
            ConnectionRequest::Postgres(data) => normalized_postgres_endpoint(data).database,
            ConnectionRequest::MySql(data) => {
                let endpoint = normalized_mysql_endpoint(data);
                if endpoint.database.is_empty() {
                    endpoint.host
                } else {
                    endpoint.database
                }
            }
            ConnectionRequest::ClickHouse(data) => data.effective_database().to_string(),
        }
    }

    pub fn identity_key(&self) -> String {
        match self {
            ConnectionRequest::Sqlite(data) => format!("sqlite:{}", data.path.trim()),
            ConnectionRequest::Postgres(data) => {
                let endpoint = normalized_postgres_endpoint(data);
                format!(
                    "postgres:{}@{}:{}/{}{}",
                    endpoint.username,
                    endpoint.host.to_ascii_lowercase(),
                    endpoint.port,
                    endpoint.database,
                    ssh_identity_suffix(data.ssh_tunnel.as_ref())
                )
            }
            ConnectionRequest::MySql(data) => {
                let endpoint = normalized_mysql_endpoint(data);
                format!(
                    "mysql:{}@{}:{}|database:{}{}",
                    endpoint.username,
                    endpoint.host.to_ascii_lowercase(),
                    endpoint.port,
                    endpoint.database,
                    ssh_identity_suffix(data.ssh_tunnel.as_ref())
                )
            }
            ConnectionRequest::ClickHouse(data) => format!(
                "clickhouse:{}|user:{}|database:{}{}",
                normalized_clickhouse_endpoint_key(data),
                data.effective_username(),
                data.effective_database(),
                ssh_identity_suffix(data.ssh_tunnel.as_ref())
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedPostgresEndpoint {
    host: String,
    port: u16,
    username: String,
    database: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedMySqlEndpoint {
    host: String,
    port: u16,
    username: String,
    database: String,
}

fn normalized_postgres_endpoint(data: &PostgresFormData) -> NormalizedPostgresEndpoint {
    if let Some(endpoint) = parsed_postgres_dsn(data.host.trim(), &data.username, &data.database) {
        return endpoint;
    }

    let host = normalized_postgres_host(&data.host);
    let port = if data.port == 0 { 5432 } else { data.port };
    let username = normalized_postgres_username(&data.username);
    let database = normalized_postgres_database(&data.database, &username);
    NormalizedPostgresEndpoint {
        host,
        port,
        username,
        database,
    }
}

fn normalized_mysql_endpoint(data: &MySqlFormData) -> NormalizedMySqlEndpoint {
    if let Some(endpoint) = parsed_mysql_dsn(data.host.trim(), &data.username, &data.database) {
        return endpoint;
    }

    let (host, embedded_port) = split_mysql_host_and_port(&data.host);
    let host = normalized_mysql_host(&host);
    let port = embedded_port.unwrap_or(if data.port == 0 { 3306 } else { data.port });
    let username = normalized_mysql_username(&data.username);
    let database = normalized_mysql_database(&data.database);
    NormalizedMySqlEndpoint {
        host,
        port,
        username,
        database,
    }
}

fn looks_like_postgres_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("postgres://") || value.starts_with("postgresql://")
}

fn normalized_postgres_host(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        "localhost".to_string()
    } else {
        host.to_string()
    }
}

fn looks_like_mysql_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("mysql://") || value.starts_with("mariadb://")
}

fn normalized_mysql_host(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        "localhost".to_string()
    } else {
        host.to_string()
    }
}

fn normalized_postgres_username(username: &str) -> String {
    let username = username.trim();
    if username.is_empty() {
        "postgres".to_string()
    } else {
        username.to_string()
    }
}

fn normalized_mysql_username(username: &str) -> String {
    let username = username.trim();
    if username.is_empty() {
        "root".to_string()
    } else {
        username.to_string()
    }
}

fn normalized_postgres_database(database: &str, username: &str) -> String {
    let database = database.trim();
    if database.is_empty() {
        username.to_string()
    } else {
        database.to_string()
    }
}

fn normalized_mysql_database(database: &str) -> String {
    database.trim().to_string()
}

fn parsed_postgres_dsn(
    value: &str,
    fallback_username: &str,
    fallback_database: &str,
) -> Option<NormalizedPostgresEndpoint> {
    if !looks_like_postgres_dsn(value) {
        return None;
    }

    let url = Url::parse(value).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port().unwrap_or(5432);
    let username = if url.username().is_empty() {
        normalized_postgres_username(fallback_username)
    } else {
        url.username().to_string()
    };
    let database = url
        .path_segments()
        .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| normalized_postgres_database(fallback_database, &username));

    Some(NormalizedPostgresEndpoint {
        host,
        port,
        username,
        database,
    })
}

fn parsed_mysql_dsn(
    value: &str,
    fallback_username: &str,
    fallback_database: &str,
) -> Option<NormalizedMySqlEndpoint> {
    if !looks_like_mysql_dsn(value) {
        return None;
    }

    let url = Url::parse(value).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port().unwrap_or(3306);
    let username = if url.username().is_empty() {
        normalized_mysql_username(fallback_username)
    } else {
        url.username().to_string()
    };
    let database = url
        .path_segments()
        .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| normalized_mysql_database(fallback_database));

    Some(NormalizedMySqlEndpoint {
        host,
        port,
        username,
        database,
    })
}

fn split_mysql_host_and_port(value: &str) -> (String, Option<u16>) {
    let value = value.trim();
    if value.is_empty() {
        return (String::new(), None);
    }

    if value.starts_with('[')
        && let Some(end_bracket) = value.find(']')
    {
        let host = value[1..end_bracket].to_string();
        let remainder = value[end_bracket + 1..].trim();
        if remainder.is_empty() {
            return (host, None);
        }
        if let Some(port) = remainder
            .strip_prefix(':')
            .and_then(|port| port.parse::<u16>().ok())
        {
            return (host, Some(port));
        }
        return (value.to_string(), None);
    }

    if value.matches(':').count() == 1
        && let Some((host, port)) = value.rsplit_once(':')
        && !host.trim().is_empty()
        && let Ok(port) = port.trim().parse::<u16>()
    {
        return (host.trim().to_string(), Some(port));
    }

    (value.to_string(), None)
}

fn normalized_clickhouse_endpoint_key(data: &ClickHouseFormData) -> String {
    if let Some(url) = parsed_clickhouse_url(&data.host) {
        let host = url
            .host_str()
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| data.host.trim().to_ascii_lowercase());
        let port = url
            .port_or_known_default()
            .unwrap_or(normalized_clickhouse_port(data));
        let path = normalized_url_path(&url);
        return format!(
            "{}://{}:{}{}",
            url.scheme().to_ascii_lowercase(),
            host,
            port,
            path
        );
    }

    format!(
        "{}:{}",
        data.host.trim().to_ascii_lowercase(),
        normalized_clickhouse_port(data)
    )
}

fn clickhouse_endpoint_label(data: &ClickHouseFormData) -> String {
    if let Some(url) = parsed_clickhouse_url(&data.host) {
        let host = url.host_str().unwrap_or(data.host.trim());
        let port = url
            .port_or_known_default()
            .unwrap_or(normalized_clickhouse_port(data));
        let path = normalized_url_path(&url);
        return format!("{}://{}:{}{}", url.scheme(), host, port, path);
    }

    format!("{}:{}", data.host.trim(), normalized_clickhouse_port(data))
}

fn normalized_clickhouse_port(data: &ClickHouseFormData) -> u16 {
    if data.port == 0 { 8123 } else { data.port }
}

fn parsed_clickhouse_url(value: &str) -> Option<Url> {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        Url::parse(value).ok()
    } else {
        None
    }
}

fn normalized_url_path(url: &Url) -> String {
    let path = url.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        String::new()
    } else {
        path.to_string()
    }
}

fn ssh_identity_suffix(config: Option<&SshTunnelConfig>) -> String {
    let Some(config) = config.filter(|config| config.is_configured()) else {
        return String::new();
    };

    let key_path = config.private_key_path.trim();
    if key_path.is_empty() {
        format!(
            "|ssh:{}@{}:{}",
            config.username.trim(),
            config.host.trim().to_ascii_lowercase(),
            config.effective_port()
        )
    } else {
        format!(
            "|ssh:{}@{}:{}|key:{}",
            config.username.trim(),
            config.host.trim().to_ascii_lowercase(),
            config.effective_port(),
            key_path
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClickHouseFormData,
        ConnectionRequest,
        MySqlFormData,
        PostgresFormData,
        SavedConnection,
        SqliteFormData,
        SshTunnelConfig,
    };

    #[test]
    fn postgres_dsn_display_name_redacts_password() {
        let request = ConnectionRequest::Postgres(PostgresFormData {
            host: "postgres://alice:super-secret@db.example.com:5433/app?sslmode=require"
                .to_string(),
            port: 5432,
            username: String::new(),
            password: "ignored".to_string(),
            database: String::new(),
            ssl_mode: "prefer".to_string(),
            ssh_tunnel: None,
        });

        assert_eq!(
            request.display_name(),
            "PostgreSQL · alice@db.example.com:5433/app"
        );
        assert_eq!(request.short_name(), "app");
        assert!(!request.display_name().contains("super-secret"));
        assert!(!request.identity_key().contains("super-secret"));
    }

    #[test]
    fn postgres_short_name_matches_runtime_default_database() {
        let request = ConnectionRequest::Postgres(PostgresFormData {
            host: "localhost".to_string(),
            port: 5432,
            username: "analytics".to_string(),
            password: String::new(),
            database: String::new(),
            ssl_mode: "prefer".to_string(),
            ssh_tunnel: None,
        });

        assert_eq!(request.short_name(), "analytics");
        assert_eq!(
            request.identity_key(),
            "postgres:analytics@localhost:5432/analytics"
        );
    }

    #[test]
    fn clickhouse_url_identity_key_redacts_credentials_and_keeps_path() {
        let request = ConnectionRequest::ClickHouse(ClickHouseFormData {
            host: "https://svc-user:super-secret@click.example.com:8443/proxy".to_string(),
            port: 8123,
            username: "default".to_string(),
            password: "top-secret".to_string(),
            database: "warehouse".to_string(),
            ssh_tunnel: Some(SshTunnelConfig {
                host: "bastion.example.com".to_string(),
                port: 22,
                username: "ops".to_string(),
                private_key_path: "/keys/prod".to_string(),
            }),
        });

        assert_eq!(
            request.display_name(),
            "ClickHouse · default@https://click.example.com:8443/proxy/warehouse via SSH ops@bastion.example.com:22"
        );
        assert!(
            request
                .identity_key()
                .contains("https://click.example.com:8443/proxy")
        );
        assert!(!request.identity_key().contains("super-secret"));
    }

    #[test]
    fn sqlite_identity_key_uses_trimmed_path() {
        let request = ConnectionRequest::Sqlite(SqliteFormData {
            path: " /tmp/app.db ".to_string(),
        });

        assert_eq!(request.display_name(), "SQLite · /tmp/app.db");
        assert_eq!(request.identity_key(), "sqlite:/tmp/app.db");
    }

    #[test]
    fn mysql_dsn_display_name_redacts_password() {
        let request = ConnectionRequest::MySql(MySqlFormData {
            host: "mysql://alice:super-secret@db.example.com:3307/app".to_string(),
            port: 3306,
            username: String::new(),
            password: "ignored".to_string(),
            database: String::new(),
            ssl_mode: "preferred".to_string(),
            ssh_tunnel: None,
        });

        assert_eq!(
            request.display_name(),
            "MySQL · alice@db.example.com:3307/app"
        );
        assert_eq!(request.short_name(), "app");
        assert!(!request.display_name().contains("super-secret"));
        assert!(!request.identity_key().contains("super-secret"));
    }

    #[test]
    fn mysql_empty_database_does_not_force_mysql_system_schema() {
        let request = ConnectionRequest::MySql(MySqlFormData {
            host: "db.internal:3307".to_string(),
            port: 3306,
            username: "app".to_string(),
            password: String::new(),
            database: String::new(),
            ssl_mode: "preferred".to_string(),
            ssh_tunnel: None,
        });

        assert_eq!(request.display_name(), "MySQL · app@db.internal:3307");
        assert_eq!(request.short_name(), "db.internal");
        assert_eq!(
            request.identity_key(),
            "mysql:app@db.internal:3307|database:"
        );
    }

    // ── Serialization round-trip tests ────────────────────────────────

    #[test]
    fn postgres_form_data_round_trips_with_ssh_tunnel() {
        let data = PostgresFormData {
            host: "db.example.com".to_string(),
            port: 5432,
            username: "admin".to_string(),
            password: "secret".to_string(),
            database: "mydb".to_string(),
            ssl_mode: "prefer".to_string(),
            ssh_tunnel: Some(SshTunnelConfig {
                host: "bastion.example.com".to_string(),
                port: 2222,
                username: "ubuntu".to_string(),
                private_key_path: "~/.ssh/id_ed25519".to_string(),
            }),
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: PostgresFormData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
        assert!(parsed.ssh_tunnel.is_some());
        let tunnel = parsed.ssh_tunnel.unwrap();
        assert_eq!(tunnel.host, "bastion.example.com");
        assert_eq!(tunnel.private_key_path, "~/.ssh/id_ed25519");
    }

    #[test]
    fn postgres_form_data_round_trips_without_ssh_tunnel() {
        let data = PostgresFormData {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: String::new(),
            database: "postgres".to_string(),
            ssl_mode: "prefer".to_string(),
            ssh_tunnel: None,
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: PostgresFormData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
        assert!(parsed.ssh_tunnel.is_none());
    }

    #[test]
    fn mysql_form_data_round_trips_with_ssh_tunnel() {
        let data = MySqlFormData {
            host: "db.example.com".to_string(),
            port: 3306,
            username: "root".to_string(),
            password: "secret".to_string(),
            database: "mydb".to_string(),
            ssl_mode: "preferred".to_string(),
            ssh_tunnel: Some(SshTunnelConfig {
                host: "bastion.example.com".to_string(),
                port: 22,
                username: "ubuntu".to_string(),
                private_key_path: String::new(),
            }),
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: MySqlFormData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
    }

    #[test]
    fn clickhouse_form_data_round_trips_with_ssh_tunnel() {
        let data = ClickHouseFormData {
            host: "http://ch.example.com:8123".to_string(),
            port: 8123,
            username: "default".to_string(),
            password: String::new(),
            database: "analytics".to_string(),
            ssh_tunnel: Some(SshTunnelConfig {
                host: "bastion.example.com".to_string(),
                port: 22,
                username: "ops".to_string(),
                private_key_path: "/home/ops/.ssh/id_rsa".to_string(),
            }),
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: ClickHouseFormData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
    }

    #[test]
    fn sqlite_form_data_round_trips() {
        let data = SqliteFormData {
            path: "/tmp/test.db".to_string(),
        };
        let json = serde_json::to_string(&data).expect("serialize");
        let parsed: SqliteFormData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, data);
    }

    #[test]
    fn connection_request_round_trips_all_variants() {
        let requests = vec![
            ConnectionRequest::Sqlite(SqliteFormData {
                path: "/data/app.db".to_string(),
            }),
            ConnectionRequest::Postgres(PostgresFormData {
                host: "localhost".to_string(),
                port: 5432,
                username: "postgres".to_string(),
                password: "pass".to_string(),
                database: "testdb".to_string(),
                ssl_mode: "prefer".to_string(),
                ssh_tunnel: None,
            }),
            ConnectionRequest::MySql(MySqlFormData {
                host: "db.example.com".to_string(),
                port: 3306,
                username: "root".to_string(),
                password: "pass".to_string(),
                database: "testdb".to_string(),
                ssl_mode: "preferred".to_string(),
                ssh_tunnel: Some(SshTunnelConfig {
                    host: "ssh.example.com".to_string(),
                    port: 22,
                    username: "deploy".to_string(),
                    private_key_path: String::new(),
                }),
            }),
            ConnectionRequest::ClickHouse(ClickHouseFormData {
                host: "http://ch.example.com".to_string(),
                port: 8123,
                username: "default".to_string(),
                password: String::new(),
                database: "default".to_string(),
                ssh_tunnel: None,
            }),
        ];

        for original in &requests {
            let json = serde_json::to_string(original).expect("serialize");
            let parsed: ConnectionRequest = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, original);
        }
    }

    #[test]
    fn saved_connection_round_trips_with_request() {
        let saved = SavedConnection {
            name: "Production DB".to_string(),
            request: ConnectionRequest::Postgres(PostgresFormData {
                host: "db.prod.example.com".to_string(),
                port: 5432,
                username: "admin".to_string(),
                password: "secret".to_string(),
                database: "production".to_string(),
                ssl_mode: "prefer".to_string(),
                ssh_tunnel: Some(SshTunnelConfig {
                    host: "bastion.prod.example.com".to_string(),
                    port: 22,
                    username: "deploy".to_string(),
                    private_key_path: "~/.ssh/prod_key".to_string(),
                }),
            }),
        };
        let json = serde_json::to_string(&saved).expect("serialize");
        let parsed: SavedConnection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, "Production DB");
        assert_eq!(parsed.request, saved.request);
    }

    // ── SSH tunnel config safety tests ────────────────────────────────

    #[test]
    fn ssh_tunnel_default_is_unconfigured() {
        let config = SshTunnelConfig::default();
        assert!(!config.is_configured());
        assert_eq!(config.host, "");
        assert_eq!(config.username, "");
        assert_eq!(config.port, 0);
        assert_eq!(config.private_key_path, "");
        assert_eq!(config.effective_port(), 22);
    }

    #[test]
    fn ssh_tunnel_requires_both_host_and_username() {
        let host_only = SshTunnelConfig {
            host: "bastion.example.com".to_string(),
            port: 22,
            username: String::new(),
            private_key_path: String::new(),
        };
        assert!(!host_only.is_configured());

        let username_only = SshTunnelConfig {
            host: String::new(),
            port: 22,
            username: "ubuntu".to_string(),
            private_key_path: String::new(),
        };
        assert!(!username_only.is_configured());

        let configured = SshTunnelConfig {
            host: "bastion.example.com".to_string(),
            port: 22,
            username: "ubuntu".to_string(),
            private_key_path: String::new(),
        };
        assert!(configured.is_configured());
    }

    #[test]
    fn ssh_tunnel_ignores_whitespace_only_fields() {
        let whitespace = SshTunnelConfig {
            host: "   ".to_string(),
            port: 22,
            username: "  ".to_string(),
            private_key_path: String::new(),
        };
        assert!(!whitespace.is_configured());
    }

    #[test]
    fn ssh_tunnel_display_name_formats_correctly() {
        let config = SshTunnelConfig {
            host: "  bastion.example.com  ".to_string(),
            port: 0,
            username: "  ubuntu  ".to_string(),
            private_key_path: String::new(),
        };
        assert_eq!(config.display_name(), "ubuntu@bastion.example.com:22");
    }

    // ── Missing ssh_tunnel deserializes as None ───────────────────────

    #[test]
    fn postgres_missing_ssh_tunnel_field_deserializes_as_none() {
        let json =
            r#"{"host":"localhost","port":5432,"username":"pg","password":"pw","database":"db"}"#;
        let parsed: PostgresFormData = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.ssh_tunnel.is_none());
    }

    #[test]
    fn mysql_missing_ssh_tunnel_field_deserializes_as_none() {
        let json =
            r#"{"host":"localhost","port":3306,"username":"root","password":"pw","database":"db"}"#;
        let parsed: MySqlFormData = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.ssh_tunnel.is_none());
    }

    #[test]
    fn clickhouse_missing_ssh_tunnel_field_deserializes_as_none() {
        let json = r#"{"host":"localhost","port":8123,"username":"default","password":"","database":"default"}"#;
        let parsed: ClickHouseFormData = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.ssh_tunnel.is_none());
    }

    // ── Missing ssl_mode falls back to the serde default ──────────────
    // Old saved connections on disk predate the ssl_mode field; the
    // #[serde(default)] must keep deserialization working.

    #[test]
    fn postgres_missing_ssl_mode_defaults_to_prefer() {
        let json =
            r#"{"host":"localhost","port":5432,"username":"pg","password":"pw","database":"db"}"#;
        let parsed: PostgresFormData = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.ssl_mode, "prefer");
    }

    #[test]
    fn mysql_missing_ssl_mode_defaults_to_preferred() {
        let json =
            r#"{"host":"localhost","port":3306,"username":"root","password":"pw","database":"db"}"#;
        let parsed: MySqlFormData = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.ssl_mode, "preferred");
    }

    #[test]
    fn postgres_explicit_ssl_mode_overrides_default() {
        let json = r#"{"host":"localhost","port":5432,"username":"pg","password":"pw","database":"db","ssl_mode":"require"}"#;
        let parsed: PostgresFormData = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.ssl_mode, "require");
    }
}
