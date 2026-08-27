//! Database connection orchestration and SSH tunnel lifecycle management for Shovel.

mod registry;
pub use database::SessionHandle;
pub use registry::{register_session, session, unregister_session};

#[cfg(feature = "sqlite")]
use std::sync::Arc;

use connection_ssh::{OpenedSshTunnel, open_ssh_tunnel, register_ssh_tunnel};
#[cfg(any(
    feature = "sqlite",
    feature = "postgres",
    feature = "mysql",
    feature = "clickhouse"
))]
use database::DatabaseDriver;
use database::LiveConnection;
#[cfg(feature = "clickhouse")]
use driver_clickhouse::ClickHouseDriver;
#[cfg(feature = "mysql")]
use driver_mysql::{MySqlConfig, MySqlDriver};
#[cfg(feature = "postgres")]
use driver_postgres::{PgConfig, PgDriver};
#[cfg(feature = "sqlite")]
use driver_sqlite::{SqliteDriver, SqliteSession};
use models::{ClickHouseFormData, ConnectionRequest, DatabaseError, SshTunnelConfig};
#[cfg(feature = "clickhouse")]
use reqwest::Url;

/// Releases an SSH tunnel that was previously opened for a connection
/// session, identified by the session's identity key.
///
/// This is a re-export from the `connection-ssh` crate. It should be
/// called when a connection session is torn down to ensure the tunnel
/// process is terminated and its local port is freed.
///
/// # Parameters
///
/// * `identity_key` — the unique session identity key that was used to
///   register the tunnel via [`connect_to_db`].
pub use connection_ssh::release_ssh_tunnel;

/// The result of SSH tunnel resolution: either an opened tunnel or none.
#[allow(dead_code)]
struct ResolvedTunnel {
    tunnel: Option<OpenedSshTunnel>,
    remote_host: String,
    remote_port: u16,
}

/// Resolves an optional SSH tunnel for database connections.
///
/// When `ssh_tunnel` is `Some` and configured, this function validates
/// the input (optional DSN check), normalizes host/port, opens the
/// tunnel, and returns [`ResolvedTunnel`] with the opened tunnel.
///
/// When `ssh_tunnel` is `None` or not configured, it returns
/// `ResolvedTunnel { tunnel: None, ... }` immediately.
async fn resolve_ssh_tunnel(
    ssh_tunnel: &Option<SshTunnelConfig>,
    host: &str,
    port: u16,
    default_port: u16,
    dsn_check: Option<fn(&str) -> bool>,
) -> Result<ResolvedTunnel, DatabaseError> {
    let config = match ssh_tunnel.as_ref() {
        Some(c) => c,
        None => {
            return Ok(ResolvedTunnel {
                tunnel: None,
                remote_host: host.to_string(),
                remote_port: if port == 0 { default_port } else { port },
            });
        }
    };

    if !config.is_configured() {
        return Err(DatabaseError::Tunnel(
            "SSH tunnel is enabled, but SSH host or username is empty".to_string(),
        ));
    }

    if let Some(check) = dsn_check
        && check(host)
    {
        return Err(DatabaseError::Tunnel(
            "SSH tunnel is not supported with DSN input. Use host and port fields.".to_string(),
        ));
    }

    let remote_host = if host.trim().is_empty() {
        "localhost".to_string()
    } else {
        host.trim().to_string()
    };
    let remote_port = if port == 0 { default_port } else { port };

    let tunnel = open_ssh_tunnel(config, &remote_host, remote_port)
        .await
        .map_err(DatabaseError::Tunnel)?;

    Ok(ResolvedTunnel {
        tunnel: Some(tunnel),
        remote_host,
        remote_port,
    })
}

/// Register the tunnel on success, release it on failure.
fn finalize_tunnel<T>(
    session_key: &str,
    resolved: ResolvedTunnel,
    result: &Result<T, DatabaseError>,
) {
    if let Some(tunnel) = resolved.tunnel {
        if result.is_ok() {
            register_ssh_tunnel(session_key.to_string(), tunnel);
        } else {
            release_ssh_tunnel(session_key);
        }
    }
}

/// Проверяет возможность подключения по [`ConnectionRequest`] без сохранения
/// конфигурации и без создания сессии. Используется кнопкой «Test connection»
/// в форме подключения.
///
/// Повторно использует [`connect_to_db`], поэтому вся маршрутизация драйверов
/// и логика SSH-туннелей остаётся единой. На успехе немедленно освобождает
/// возможно открытый SSH-туннель через [`release_ssh_tunnel`], чтобы
/// тестовое подключение не оставляло висящий ssh-процесс и не засоряло
/// глобальный реестр туннелей. На неудаче туннель уже освобождается внутри
/// [`connect_to_db`], поэтому повторно его трогать не нужно.
///
/// # Ошибки
///
/// Возвращает [`DatabaseError`], если не удалось подключиться — те же случаи,
/// что и у [`connect_to_db`].
pub async fn test_connection(request: ConnectionRequest) -> Result<(), DatabaseError> {
    let session_key = request.identity_key();
    let connection = connect_to_db(request).await?;
    // Сначала закрываем пул соединений, иначе активные соединения в пуле
    // потеряют транспорт при остановке туннеля.
    drop(connection);
    release_ssh_tunnel(&session_key);
    Ok(())
}

/// Establishes a live database connection for the given
/// [`ConnectionRequest`], dispatching to the appropriate backend-specific
/// driver and optionally setting up an SSH tunnel.
///
/// This is the single entrypoint for all database connections in the
/// application. It handles:
///
/// - **SQLite**: connects directly to the local file path.
/// - **PostgreSQL**: connects directly or via SSH tunnel; rejects DSN-style
///   host strings when tunneling is requested.
/// - **MySQL**: connects directly or via SSH tunnel; supports IPv6 host
///   notation with optional embedded port (e.g. `[::1]:3306`); rejects
///   DSN-style host strings when tunneling.
/// - **ClickHouse**: connects directly or via SSH tunnel; supports plain
///   host, `http://` URL, and `https://` URL host formats (HTTPS is
///   rejected when tunneling because TLS host validation would target
///   `127.0.0.1`).
///
/// # SSH tunnel lifecycle
///
/// When an SSH tunnel is configured and the connection succeeds, the tunnel
/// is registered under the session's identity key via
/// [`register_ssh_tunnel`]. If the connection fails after the tunnel was
/// opened, the tunnel is immediately released via
/// [`release_ssh_tunnel`] to avoid leaking resources.
///
/// # Parameters
///
/// * `request` — a [`ConnectionRequest`] enum variant carrying the
///   backend-specific connection form data (host, port, credentials,
///   optional SSH tunnel config).
///
/// # Returns
///
/// * `Ok(SessionHandle)` — a type-erased session wrapping the live connection
///   (`SqliteSession` for SQLite; other backends still use
///   [`SessionHandle::from_legacy`]).
///
/// # Errors
///
/// Returns [`DatabaseError`] in the following cases:
///
/// * `DatabaseError::Driver` — a backend driver failed to connect (bad
///   credentials, unreachable host, missing file, HTTP error, etc.).
/// * `DatabaseError::Tunnel` — SSH tunnel configuration is invalid (e.g.
///   host or username is empty, DSN input used with tunneling, HTTPS
///   ClickHouse endpoint requested over SSH, unparseable ClickHouse URL).
/// * `DatabaseError::Unsupported` — the requested driver is not compiled in.
pub async fn connect_to_db(request: ConnectionRequest) -> Result<SessionHandle, DatabaseError> {
    let session_key = request.identity_key();

    match request {
        #[cfg(feature = "sqlite")]
        ConnectionRequest::Sqlite(data) => {
            let pool = SqliteDriver::connect(data.path)
                .await
                .map_err(|e| DatabaseError::Driver(e.to_string()))?;
            Ok(SessionHandle::wrap(Arc::new(SqliteSession { pool })))
        }
        #[cfg(feature = "postgres")]
        ConnectionRequest::Postgres(mut data) => {
            let resolved = resolve_ssh_tunnel(
                &data.ssh_tunnel,
                &data.host,
                data.port,
                5432,
                Some(looks_like_postgres_dsn),
            )
            .await?;

            if let Some(ref tunnel) = resolved.tunnel {
                data.host = "127.0.0.1".to_string();
                data.port = tunnel.local_port;
            }

            let result = async {
                let config = PgConfig {
                    host: data.host.clone(),
                    port: data.port,
                    username: data.username.clone(),
                    password: data.password.clone(),
                    database: data.database.clone(),
                    ssl_mode: data.ssl_mode.clone(),
                };
                PgDriver::connect(config)
                    .await
                    .map_err(|e| DatabaseError::Driver(e.to_string()))
                    .map(LiveConnection::Postgres)
            }
            .await;

            finalize_tunnel(&session_key, resolved, &result);
            result.map(SessionHandle::from_legacy)
        }
        #[cfg(feature = "mysql")]
        ConnectionRequest::MySql(mut data) => {
            let (host_for_tunnel, embedded_port) = split_mysql_host_and_port(&data.host);
            let effective_port =
                embedded_port.unwrap_or(if data.port == 0 { 3306 } else { data.port });

            let resolved = resolve_ssh_tunnel(
                &data.ssh_tunnel,
                &host_for_tunnel,
                effective_port,
                3306,
                Some(looks_like_mysql_dsn),
            )
            .await?;

            if let Some(ref tunnel) = resolved.tunnel {
                data.host = "127.0.0.1".to_string();
                data.port = tunnel.local_port;
            }

            let result = async {
                let config = MySqlConfig {
                    host: data.host.clone(),
                    port: data.port,
                    username: data.username.clone(),
                    password: data.password.clone(),
                    database: data.database.clone(),
                    ssl_mode: data.ssl_mode.clone(),
                };
                MySqlDriver::connect(config)
                    .await
                    .map_err(|e| DatabaseError::Driver(e.to_string()))
                    .map(LiveConnection::MySql)
            }
            .await;

            finalize_tunnel(&session_key, resolved, &result);
            result.map(SessionHandle::from_legacy)
        }
        #[cfg(feature = "clickhouse")]
        ConnectionRequest::ClickHouse(mut data) => {
            let resolved = if let Some(config) = data.ssh_tunnel.as_ref() {
                if !config.is_configured() {
                    return Err(DatabaseError::Tunnel(
                        "SSH tunnel is enabled, but SSH host or username is empty".to_string(),
                    ));
                }

                let target = parse_clickhouse_target(&data)?;
                let tunnel = open_ssh_tunnel(config, &target.remote_host, target.remote_port)
                    .await
                    .map_err(DatabaseError::Tunnel)?;
                data.host = target.connect_host(tunnel.local_port);
                data.port = tunnel.local_port;
                ResolvedTunnel {
                    tunnel: Some(tunnel),
                    remote_host: target.remote_host,
                    remote_port: target.remote_port,
                }
            } else {
                ResolvedTunnel {
                    tunnel: None,
                    remote_host: String::new(),
                    remote_port: 0,
                }
            };

            let result = async {
                ClickHouseDriver::connect(data.clone())
                    .await
                    .map_err(DatabaseError::Driver)
                    .map(LiveConnection::ClickHouse)
            }
            .await;

            finalize_tunnel(&session_key, resolved, &result);
            result.map(SessionHandle::from_legacy)
        }
        #[allow(unreachable_patterns)]
        _ => Err(DatabaseError::Unsupported(
            "this database driver is not compiled in".to_string(),
        )),
    }
}

#[cfg(feature = "postgres")]
fn looks_like_postgres_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("postgres://") || value.starts_with("postgresql://")
}

#[cfg(feature = "mysql")]
fn looks_like_mysql_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("mysql://") || value.starts_with("mariadb://")
}

#[cfg(feature = "mysql")]
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

#[cfg(feature = "clickhouse")]
#[derive(Debug)]
struct ClickHouseTarget {
    remote_host: String,
    remote_port: u16,
    scheme: Option<String>,
}

#[cfg(feature = "clickhouse")]
impl ClickHouseTarget {
    fn connect_host(&self, local_port: u16) -> String {
        match self.scheme.as_deref() {
            Some(scheme) => format!("{scheme}://127.0.0.1:{local_port}"),
            None => "127.0.0.1".to_string(),
        }
    }
}

#[cfg(feature = "clickhouse")]
fn parse_clickhouse_target(data: &ClickHouseFormData) -> Result<ClickHouseTarget, DatabaseError> {
    let host = data.host.trim();
    if host.is_empty() {
        return Err(DatabaseError::Tunnel(
            "ClickHouse host is empty, nothing to tunnel to".to_string(),
        ));
    }

    if host.starts_with("http://") || host.starts_with("https://") {
        let url = Url::parse(host)
            .map_err(|err| DatabaseError::Tunnel(format!("invalid ClickHouse URL: {err}")))?;
        if url.scheme().eq_ignore_ascii_case("https") {
            return Err(DatabaseError::Tunnel(
                "ClickHouse HTTPS endpoints are not supported through the current SSH tunnel implementation because TLS host validation would target 127.0.0.1. Use an HTTP endpoint over SSH or connect directly.".to_string(),
            ));
        }
        let remote_host = url
            .host_str()
            .ok_or_else(|| DatabaseError::Tunnel("ClickHouse URL has no host".to_string()))?
            .to_string();
        let remote_port = url
            .port()
            .unwrap_or(if data.port == 0 { 8123 } else { data.port });
        return Ok(ClickHouseTarget {
            remote_host,
            remote_port,
            scheme: Some(url.scheme().to_string()),
        });
    }

    Ok(ClickHouseTarget {
        remote_host: host.to_string(),
        remote_port: if data.port == 0 { 8123 } else { data.port },
        scheme: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── looks_like_postgres_dsn ──────────────────────────────────────

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_dsn_detects_postgres_scheme() {
        assert!(looks_like_postgres_dsn("postgres://user@host/db"));
        assert!(looks_like_postgres_dsn("POSTGRES://user@host/db"));
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_dsn_detects_postgresql_scheme() {
        assert!(looks_like_postgres_dsn("postgresql://user@host/db"));
        assert!(looks_like_postgres_dsn("PostgreSQL://user@host/db"));
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_dsn_rejects_non_dsn() {
        assert!(!looks_like_postgres_dsn("localhost"));
        assert!(!looks_like_postgres_dsn("http://example.com"));
        assert!(!looks_like_postgres_dsn(""));
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_dsn_ignores_leading_whitespace() {
        assert!(looks_like_postgres_dsn("  postgres://host/db"));
    }

    // ── looks_like_mysql_dsn ─────────────────────────────────────────

    #[cfg(feature = "mysql")]
    #[test]
    fn mysql_dsn_detects_mysql_scheme() {
        assert!(looks_like_mysql_dsn("mysql://user@host/db"));
        assert!(looks_like_mysql_dsn("MYSQL://user@host/db"));
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn mysql_dsn_detects_mariadb_scheme() {
        assert!(looks_like_mysql_dsn("mariadb://user@host/db"));
        assert!(looks_like_mysql_dsn("MariaDB://user@host/db"));
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn mysql_dsn_rejects_non_dsn() {
        assert!(!looks_like_mysql_dsn("localhost"));
        assert!(!looks_like_mysql_dsn("postgres://host"));
        assert!(!looks_like_mysql_dsn(""));
    }

    // ── split_mysql_host_and_port ────────────────────────────────────

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_standard() {
        assert_eq!(
            split_mysql_host_and_port("db.example.com:3307"),
            ("db.example.com".to_string(), Some(3307))
        );
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_no_port() {
        assert_eq!(
            split_mysql_host_and_port("db.example.com"),
            ("db.example.com".to_string(), None)
        );
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_ipv6_with_port() {
        assert_eq!(
            split_mysql_host_and_port("[::1]:4406"),
            ("::1".to_string(), Some(4406))
        );
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_ipv6_without_port() {
        assert_eq!(
            split_mysql_host_and_port("[::1]"),
            ("::1".to_string(), None)
        );
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_empty() {
        assert_eq!(split_mysql_host_and_port(""), (String::new(), None));
        assert_eq!(split_mysql_host_and_port("  "), (String::new(), None));
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_multiple_colons() {
        // IPv6 without brackets – too many colons, treated as opaque
        let (host, port) = split_mysql_host_and_port("::1");
        assert_eq!(host, "::1");
        assert_eq!(port, None);
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn split_mysql_host_and_port_invalid_port() {
        assert_eq!(
            split_mysql_host_and_port("host:notaport"),
            ("host:notaport".to_string(), None)
        );
    }

    // ── ClickHouseTarget::connect_host ───────────────────────────────

    #[test]
    fn clickhouse_connect_host_with_scheme() {
        let target = ClickHouseTarget {
            remote_host: "ch.example.com".to_string(),
            remote_port: 8123,
            scheme: Some("http".to_string()),
        };
        assert_eq!(target.connect_host(9999), "http://127.0.0.1:9999");
    }

    #[test]
    fn clickhouse_connect_host_without_scheme() {
        let target = ClickHouseTarget {
            remote_host: "ch.example.com".to_string(),
            remote_port: 8123,
            scheme: None,
        };
        assert_eq!(target.connect_host(9999), "127.0.0.1");
    }

    // ── parse_clickhouse_target ──────────────────────────────────────

    fn ch_form(host: &str, port: u16) -> ClickHouseFormData {
        ClickHouseFormData {
            host: host.to_string(),
            port,
            username: "default".to_string(),
            password: String::new(),
            database: "default".to_string(),
            ssh_tunnel: None,
        }
    }

    #[test]
    fn parse_clickhouse_target_empty_host_is_error() {
        let result = parse_clickhouse_target(&ch_form("", 8123));
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            DatabaseError::Tunnel(msg) => assert!(msg.contains("empty")),
            other => panic!("expected Tunnel error, got {other:?}"),
        }
    }

    #[test]
    fn parse_clickhouse_target_plain_host() {
        let result = parse_clickhouse_target(&ch_form("ch.example.com", 8123));
        let target = result.unwrap();
        assert_eq!(target.remote_host, "ch.example.com");
        assert_eq!(target.remote_port, 8123);
        assert!(target.scheme.is_none());
    }

    #[test]
    fn parse_clickhouse_target_plain_host_default_port() {
        let result = parse_clickhouse_target(&ch_form("ch.example.com", 0));
        let target = result.unwrap();
        assert_eq!(target.remote_port, 8123);
    }

    #[test]
    fn parse_clickhouse_target_http_url() {
        let result = parse_clickhouse_target(&ch_form("http://ch.example.com:9000", 0));
        let target = result.unwrap();
        assert_eq!(target.remote_host, "ch.example.com");
        assert_eq!(target.remote_port, 9000);
        assert_eq!(target.scheme.as_deref(), Some("http"));
    }

    #[test]
    fn parse_clickhouse_target_http_url_default_port() {
        let result = parse_clickhouse_target(&ch_form("http://ch.example.com", 0));
        let target = result.unwrap();
        assert_eq!(target.remote_port, 8123);
    }

    #[test]
    fn parse_clickhouse_target_https_url_is_error() {
        let result = parse_clickhouse_target(&ch_form("https://ch.example.com", 0));
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            DatabaseError::Tunnel(msg) => assert!(msg.contains("HTTPS")),
            other => panic!("expected Tunnel error about HTTPS, got {other:?}"),
        }
    }

    #[test]
    fn parse_clickhouse_target_invalid_url_is_error() {
        let result = parse_clickhouse_target(&ch_form("http://[invalid", 0));
        assert!(result.is_err());
    }

    // ── connect_to_db (integration note) ────────────────────────────

    #[test]
    fn connect_to_db_requires_live_databases() {
        // `connect_to_db` dispatches to driver-specific connect calls that
        // require live database servers. It should be tested via integration
        // tests with actual database instances or Docker containers.
        //
        // The pure helper functions tested above cover all the routing and
        // validation logic that runs before any network call.
    }
}
