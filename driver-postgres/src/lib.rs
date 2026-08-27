mod rows;
mod session;

pub use session::PostgresSession;

use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::str::FromStr;

#[derive(Debug)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    pub ssl_mode: String,
}

pub struct PgDriver {}
impl database::DatabaseDriver for PgDriver {
    type Config = PgConfig;
    type Pool = sqlx::PgPool;
    type Error = sqlx::Error;

    async fn connect(info: Self::Config) -> Result<Self::Pool, Self::Error> {
        if looks_like_dsn(&info.host) {
            let options = PgConnectOptions::from_str(info.host.trim())?;
            return Self::Pool::connect_with(options).await;
        }

        let host = normalized_host(&info.host);
        let username = normalized_username(&info.username);
        let database = normalized_database(&info.database, &username);
        let port = if info.port == 0 { 5432 } else { info.port };

        let mut attempts = vec![host.clone()];
        if host == "localhost" {
            attempts.push("127.0.0.1".to_string());
        }

        let mut last_error = None;
        for attempt_host in attempts {
            let options = PgConnectOptions::new_without_pgpass()
                .host(&attempt_host)
                .port(port)
                .username(&username)
                .password(&info.password)
                .database(&database)
                .ssl_mode(parse_pg_ssl_mode(&info.ssl_mode));

            match Self::Pool::connect_with(options).await {
                Ok(pool) => return Ok(pool),
                Err(err) => last_error = Some(err),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            sqlx::Error::Protocol("postgres connection attempts produced no result".into())
        }))
    }
}

fn looks_like_dsn(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("postgres://") || value.starts_with("postgresql://")
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
        "postgres".to_string()
    } else {
        username.to_string()
    }
}

fn normalized_database(database: &str, username: &str) -> String {
    let database = database.trim();
    if database.is_empty() {
        username.to_string()
    } else {
        database.to_string()
    }
}

fn parse_pg_ssl_mode(mode: &str) -> PgSslMode {
    match mode.trim().to_ascii_lowercase().as_str() {
        "disable" => PgSslMode::Disable,
        "allow" => PgSslMode::Allow,
        "prefer" => PgSslMode::Prefer,
        "require" => PgSslMode::Require,
        "verify-ca" => PgSslMode::VerifyCa,
        "verify-full" => PgSslMode::VerifyFull,
        _ => PgSslMode::Prefer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_session_is_a_driver_session() {
        fn assert_session<T: database::DriverSession>() {}
        assert_session::<crate::PostgresSession>();
    }

    // ── looks_like_dsn ───────────────────────────────────────────────

    #[test]
    fn dsn_detects_postgres_scheme() {
        assert!(looks_like_dsn("postgres://user@host/db"));
        assert!(looks_like_dsn("POSTGRES://user@host/db"));
    }

    #[test]
    fn dsn_detects_postgresql_scheme() {
        assert!(looks_like_dsn("postgresql://user@host/db"));
        assert!(looks_like_dsn("PostgreSQL://user@host/db"));
    }

    #[test]
    fn dsn_rejects_non_dsn() {
        assert!(!looks_like_dsn("localhost"));
        assert!(!looks_like_dsn("http://example.com"));
        assert!(!looks_like_dsn(""));
        assert!(!looks_like_dsn("mysql://host/db"));
    }

    #[test]
    fn dsn_ignores_leading_whitespace() {
        assert!(looks_like_dsn("  postgres://host/db"));
    }

    #[test]
    fn dsn_is_case_insensitive() {
        assert!(looks_like_dsn("Postgres://host/db"));
        assert!(looks_like_dsn("POSTGRESQL://host/db"));
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
    fn normalized_username_defaults_to_postgres() {
        assert_eq!(normalized_username(""), "postgres");
        assert_eq!(normalized_username("   "), "postgres");
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
    fn normalized_database_falls_back_to_username() {
        assert_eq!(normalized_database("", "admin"), "admin");
        assert_eq!(normalized_database("   ", "admin"), "admin");
    }

    #[test]
    fn normalized_database_uses_database_when_provided() {
        assert_eq!(normalized_database("mydb", "admin"), "mydb");
    }

    #[test]
    fn normalized_database_trims_input() {
        assert_eq!(normalized_database("  mydb  ", "admin"), "mydb");
    }

    #[test]
    fn normalized_database_prefers_database_over_username() {
        // When database is non-empty after trimming, it should be used
        assert_eq!(normalized_database("production", "admin"), "production");
    }

    // ── parse_pg_ssl_mode ────────────────────────────────────────────

    #[test]
    fn pg_ssl_mode_disable() {
        assert!(matches!(parse_pg_ssl_mode("disable"), PgSslMode::Disable));
    }

    #[test]
    fn pg_ssl_mode_allow() {
        assert!(matches!(parse_pg_ssl_mode("allow"), PgSslMode::Allow));
    }

    #[test]
    fn pg_ssl_mode_prefer() {
        assert!(matches!(parse_pg_ssl_mode("prefer"), PgSslMode::Prefer));
    }

    #[test]
    fn pg_ssl_mode_require() {
        assert!(matches!(parse_pg_ssl_mode("require"), PgSslMode::Require));
    }

    #[test]
    fn pg_ssl_mode_verify_ca() {
        assert!(matches!(
            parse_pg_ssl_mode("verify-ca"),
            PgSslMode::VerifyCa
        ));
    }

    #[test]
    fn pg_ssl_mode_verify_full() {
        assert!(matches!(
            parse_pg_ssl_mode("verify-full"),
            PgSslMode::VerifyFull
        ));
    }

    #[test]
    fn pg_ssl_mode_case_insensitive() {
        assert!(matches!(parse_pg_ssl_mode("REQUIRE"), PgSslMode::Require));
        assert!(matches!(parse_pg_ssl_mode("Disable"), PgSslMode::Disable));
    }

    #[test]
    fn pg_ssl_mode_trims_whitespace() {
        assert!(matches!(
            parse_pg_ssl_mode("  require  "),
            PgSslMode::Require
        ));
    }

    #[test]
    fn pg_ssl_mode_unknown_defaults_to_prefer() {
        assert!(matches!(parse_pg_ssl_mode(""), PgSslMode::Prefer));
        assert!(matches!(parse_pg_ssl_mode("nonsense"), PgSslMode::Prefer));
        // MySQL-style values should NOT cross over to Postgres.
        assert!(matches!(parse_pg_ssl_mode("disabled"), PgSslMode::Prefer));
    }
}
