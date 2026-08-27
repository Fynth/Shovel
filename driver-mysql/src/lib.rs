mod explain;
mod introspect;
mod mutate;
mod rows;
mod schema;
mod session;

pub use session::MysqlSession;

use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};
use std::str::FromStr;

#[derive(Debug)]
pub struct MySqlConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    pub ssl_mode: String,
}

pub struct MySqlDriver;

impl database::DatabaseDriver for MySqlDriver {
    type Config = MySqlConfig;
    type Pool = sqlx::MySqlPool;
    type Error = sqlx::Error;

    async fn connect(info: Self::Config) -> Result<Self::Pool, Self::Error> {
        if looks_like_dsn(&info.host) {
            let options = MySqlConnectOptions::from_str(info.host.trim())?;
            return Self::Pool::connect_with(options).await;
        }

        let (host, embedded_port) = split_host_and_port(&info.host);
        let host = normalized_host(&host);
        let username = normalized_username(&info.username);
        let database = normalized_database(&info.database);
        let port = embedded_port.unwrap_or(if info.port == 0 { 3306 } else { info.port });

        let mut attempts = vec![host.clone()];
        if host == "localhost" {
            attempts.push("127.0.0.1".to_string());
        }

        let mut last_error = None;
        for attempt_host in attempts {
            let mut options = MySqlConnectOptions::new()
                .host(&attempt_host)
                .port(port)
                .username(&username)
                .password(&info.password)
                .ssl_mode(parse_mysql_ssl_mode(&info.ssl_mode));
            if !database.is_empty() {
                options = options.database(&database);
            }

            match Self::Pool::connect_with(options).await {
                Ok(pool) => return Ok(pool),
                Err(err) => last_error = Some(err),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            sqlx::Error::Protocol("mysql connection attempts produced no result".into())
        }))
    }
}

fn looks_like_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("mysql://") || value.starts_with("mariadb://")
}

fn normalized_host(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        "localhost".to_string()
    } else {
        host.to_string()
    }
}

fn normalized_username(username: &str) -> String {
    let username = username.trim();
    if username.is_empty() {
        "root".to_string()
    } else {
        username.to_string()
    }
}

fn normalized_database(database: &str) -> String {
    database.trim().to_string()
}

fn split_host_and_port(value: &str) -> (String, Option<u16>) {
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

fn parse_mysql_ssl_mode(mode: &str) -> MySqlSslMode {
    match mode.trim().to_ascii_lowercase().as_str() {
        "disabled" => MySqlSslMode::Disabled,
        "preferred" => MySqlSslMode::Preferred,
        "required" => MySqlSslMode::Required,
        "verify-ca" => MySqlSslMode::VerifyCa,
        "verify-identity" => MySqlSslMode::VerifyIdentity,
        _ => MySqlSslMode::Preferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_session_is_a_driver_session() {
        fn assert_session<T: database::DriverSession>() {}
        assert_session::<crate::MysqlSession>();
    }

    // ── looks_like_dsn ───────────────────────────────────────────────

    #[test]
    fn dsn_detects_mysql_scheme() {
        assert!(looks_like_dsn("mysql://user@host/db"));
        assert!(looks_like_dsn("MYSQL://user@host/db"));
    }

    #[test]
    fn dsn_detects_mariadb_scheme() {
        assert!(looks_like_dsn("mariadb://user@host/db"));
        assert!(looks_like_dsn("MariaDB://user@host/db"));
    }

    #[test]
    fn dsn_rejects_non_dsn() {
        assert!(!looks_like_dsn("localhost"));
        assert!(!looks_like_dsn("http://example.com"));
        assert!(!looks_like_dsn(""));
        assert!(!looks_like_dsn("postgres://host/db"));
    }

    #[test]
    fn dsn_ignores_leading_whitespace() {
        assert!(looks_like_dsn("  mysql://host/db"));
    }

    #[test]
    fn dsn_is_case_insensitive() {
        assert!(looks_like_dsn("MySQL://host/db"));
        assert!(looks_like_dsn("MARIADB://host/db"));
    }

    // ── normalized_host ──────────────────────────────────────────────

    #[test]
    fn normalized_host_defaults_to_localhost() {
        assert_eq!(normalized_host(""), "localhost");
        assert_eq!(normalized_host("   "), "localhost");
    }

    #[test]
    fn normalized_host_trims_input() {
        assert_eq!(normalized_host("  db.example.com  "), "db.example.com");
    }

    #[test]
    fn normalized_host_preserves_non_empty_value() {
        assert_eq!(normalized_host("192.168.1.1"), "192.168.1.1");
        assert_eq!(normalized_host("db.example.com"), "db.example.com");
    }

    // ── normalized_username ──────────────────────────────────────────

    #[test]
    fn normalized_username_defaults_to_root() {
        assert_eq!(normalized_username(""), "root");
        assert_eq!(normalized_username("   "), "root");
    }

    #[test]
    fn normalized_username_trims_input() {
        assert_eq!(normalized_username("  admin  "), "admin");
    }

    #[test]
    fn normalized_username_preserves_non_empty_value() {
        assert_eq!(normalized_username("myuser"), "myuser");
    }

    // ── normalized_database ──────────────────────────────────────────

    #[test]
    fn normalized_database_trims_input() {
        assert_eq!(normalized_database("  mydb  "), "mydb");
    }

    #[test]
    fn normalized_database_preserves_empty_as_empty() {
        // Unlike Postgres, MySQL normalized_database returns "" for empty input
        assert_eq!(normalized_database(""), "");
        assert_eq!(normalized_database("   "), "");
    }

    #[test]
    fn normalized_database_preserves_value() {
        assert_eq!(normalized_database("production"), "production");
    }

    // ── split_host_and_port ──────────────────────────────────────────

    #[test]
    fn splits_mysql_host_with_embedded_port() {
        assert_eq!(
            split_host_and_port("db.example.com:3307"),
            ("db.example.com".to_string(), Some(3307))
        );
        assert_eq!(
            split_host_and_port("[::1]:4406"),
            ("::1".to_string(), Some(4406))
        );
        assert_eq!(
            split_host_and_port("db.example.com"),
            ("db.example.com".to_string(), None)
        );
    }

    #[test]
    fn splits_mysql_host_ipv6_without_port() {
        assert_eq!(split_host_and_port("[::1]"), ("::1".to_string(), None));
    }

    #[test]
    fn splits_mysql_host_empty_input() {
        assert_eq!(split_host_and_port(""), (String::new(), None));
        assert_eq!(split_host_and_port("  "), (String::new(), None));
    }

    #[test]
    fn splits_mysql_host_multiple_colons_no_brackets() {
        // IPv6 literal without brackets has multiple colons, treated as opaque
        let (host, port) = split_host_and_port("::1");
        assert_eq!(host, "::1");
        assert_eq!(port, None);
    }

    #[test]
    fn splits_mysql_host_invalid_port() {
        assert_eq!(
            split_host_and_port("host:notaport"),
            ("host:notaport".to_string(), None)
        );
    }

    // ── parse_mysql_ssl_mode ─────────────────────────────────────────

    #[test]
    fn mysql_ssl_mode_disabled() {
        assert!(matches!(
            parse_mysql_ssl_mode("disabled"),
            MySqlSslMode::Disabled
        ));
    }

    #[test]
    fn mysql_ssl_mode_preferred() {
        assert!(matches!(
            parse_mysql_ssl_mode("preferred"),
            MySqlSslMode::Preferred
        ));
    }

    #[test]
    fn mysql_ssl_mode_required() {
        assert!(matches!(
            parse_mysql_ssl_mode("required"),
            MySqlSslMode::Required
        ));
    }

    #[test]
    fn mysql_ssl_mode_verify_ca() {
        assert!(matches!(
            parse_mysql_ssl_mode("verify-ca"),
            MySqlSslMode::VerifyCa
        ));
    }

    #[test]
    fn mysql_ssl_mode_verify_identity() {
        assert!(matches!(
            parse_mysql_ssl_mode("verify-identity"),
            MySqlSslMode::VerifyIdentity
        ));
    }

    #[test]
    fn mysql_ssl_mode_case_insensitive() {
        assert!(matches!(
            parse_mysql_ssl_mode("REQUIRED"),
            MySqlSslMode::Required
        ));
        assert!(matches!(
            parse_mysql_ssl_mode("Disabled"),
            MySqlSslMode::Disabled
        ));
    }

    #[test]
    fn mysql_ssl_mode_trims_whitespace() {
        assert!(matches!(
            parse_mysql_ssl_mode("  required  "),
            MySqlSslMode::Required
        ));
    }

    #[test]
    fn mysql_ssl_mode_unknown_defaults_to_preferred() {
        assert!(matches!(parse_mysql_ssl_mode(""), MySqlSslMode::Preferred));
        assert!(matches!(
            parse_mysql_ssl_mode("nonsense"),
            MySqlSslMode::Preferred
        ));
        assert!(matches!(
            parse_mysql_ssl_mode("tls"),
            MySqlSslMode::Preferred
        ));
    }
}
