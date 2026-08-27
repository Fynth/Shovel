//! Common database driver traits and error types used by all Shovel database
//! drivers.
//!
//! This crate defines the [`DatabaseDriver`] trait — the core abstraction that
//! every database backend (SQLite, PostgreSQL, MySQL, ClickHouse) must
//! implement. Each driver crate in the workspace (`driver-sqlite`,
//! `driver-postgres`, `driver-mysql`, `driver-clickhouse`) provides its own
//! implementation of this trait, supplying a concrete pool type, error type,
//! and configuration type.
//!
//! # Architecture
//!
//! The [`DatabaseDriver`] trait is deliberately minimal: it defines only a
//! single async `connect` method. This keeps the contract between the
//! connection layer and individual drivers as simple as possible. Higher-level
//! functionality — query execution, schema exploration, row editing — is built
//! on top of the pool type returned by `connect` and lives in crates such as
//! `query-core`, `explorer`, and `connection`.
//!
//! # Associated types
//!
//! Each implementation of [`DatabaseDriver`] specifies three associated types:
//!
//! * [`Pool`](DatabaseDriver::Pool) — A managed pool of database connections
//!   (e.g. `r2d2::Pool<SqliteConnectionManager>` for SQLite,
//!   `deadpool_postgres::Pool` for PostgreSQL).
//! * [`Error`](DatabaseDriver::Error) — The error type returned by the driver’s
//!   connection logic.
//! * [`Config`](DatabaseDriver::Config) — The driver-specific configuration
//!   type that aggregates host, port, credentials, TLS settings, and any other
//!   parameters needed to open a connection.
//!
//! # Ownership
//!
//! All pool, error, and config types are owned by the implementing crate. This
//! crate does not expose any concrete error enum or configuration struct; it
//! only publishes the trait contract.

mod dialect;
pub use dialect::*;

mod handle;
pub use handle::*;

#[cfg(feature = "fake")]
mod fake;
#[cfg(feature = "fake")]
pub use fake::*;

/// A generic trait for establishing a connection pool to a database.
///
/// `DatabaseDriver` is the primary abstraction at the boundary between the
/// connection-orchestration layer and the individual database backends.
/// Each driver crate provides one implementation (e.g. `SqliteDriver`,
/// `PostgresDriver`, `MySqlDriver`, `ClickHouseDriver`).
///
/// # Type parameters (associated types)
///
/// * [`Pool`](DatabaseDriver::Pool) — The type of the connection pool.
///   Implementations typically wrap a third-party pooling crate such as
///   `r2d2`, `deadpool`, or `bb8`.
/// * [`Error`](DatabaseDriver::Error) — The error type for connection
///   operations. Drivers may define their own error enum or reuse one from
///   an underlying client library.
/// * [`Config`](DatabaseDriver::Config) — Driver-specific configuration. This
///   must be enough information to locate and authenticate against a database
///   instance. For example, a PostgreSQL configuration might include host,
///   port, database name, user, password, and SSL mode.
///
/// # Errors
///
/// The [`connect`](DatabaseDriver::connect) method returns
/// [`Err(Self::Error)`](DatabaseDriver::Error) when it cannot establish a
/// connection to the database. This can happen for many reasons: invalid
/// credentials, unreachable host, network timeouts, TLS handshake failures,
/// or an invalid connection string. The caller is expected to propagate or
/// log the error appropriately.
#[allow(async_fn_in_trait)]
pub trait DatabaseDriver {
    /// The type of the connection pool managed by this driver.
    ///
    /// This is usually a pool from a third-party crate such as
    /// `r2d2::Pool<M>` or `deadpool::managed::Pool<M>`, where the manager
    /// (`M`) wraps the underlying database client.
    type Pool;

    /// The error type for connection-related failures.
    ///
    /// This type should implement [`std::error::Error`] so that callers can
    /// use `?` or `anyhow::Error`-compatible propagation. The concrete type
    /// is driver-specific; for example, `deadpool_postgres::CreatePoolError`
    /// or a custom enum wrapping multiple failure modes.
    type Error;

    /// The configuration type required to connect to a database.
    ///
    /// This struct contains all parameters needed to open a connection:
    /// host, port, database name, credentials, TLS options, and any
    /// driver-specific settings. It is typically deserialized from saved
    /// connection metadata in the Shovel UI.
    type Config;

    /// Attempt to connect to a database and return a connection pool.
    ///
    /// This is called once per connection session. The returned pool can
    /// then be used to execute queries, inspect schemas, and perform other
    /// database operations via the driver-specific client API.
    ///
    /// # Errors
    ///
    /// Returns `Err(Self::Error)` if the driver cannot establish a connection
    /// to the database. Common failure scenarios include:
    ///
    /// * Invalid or missing credentials
    /// * Host unreachable or DNS resolution failure
    /// * Network timeout
    /// * TLS or SSL handshake failure
    /// * Invalid connection string or DSN format
    /// * Database does not exist or is not accessible
    async fn connect(info: Self::Config) -> Result<Self::Pool, Self::Error>;

    /// Execute a SQL query against ClickHouse and return the parsed JSON
    /// response.
    ///
    /// The default implementation returns
    /// [`DatabaseError::UnsupportedDriver`]. Drivers that support
    /// ClickHouse-style HTTP query execution must override this.
    async fn execute_json_query(
        &self,
        _config: &models::ClickHouseFormData,
        _sql: &str,
    ) -> Result<models::ClickHouseJsonResponse, models::DatabaseError> {
        Err(models::DatabaseError::UnsupportedDriver(
            "execute_json_query is not supported for this driver".to_string(),
        ))
    }

    /// Execute a SQL query against ClickHouse and return the raw text
    /// response.
    ///
    /// The default implementation returns
    /// [`DatabaseError::UnsupportedDriver`]. Drivers that support
    /// ClickHouse-style HTTP query execution must override this.
    async fn execute_text_query(
        &self,
        _config: &models::ClickHouseFormData,
        _sql: &str,
    ) -> Result<String, models::DatabaseError> {
        Err(models::DatabaseError::UnsupportedDriver(
            "execute_text_query is not supported for this driver".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    // This crate defines only the `DatabaseDriver` trait with no associated
    // logic or default methods. There are no pure functions to unit-test.
    //
    // Each driver crate (driver-sqlite, driver-postgres, driver-mysql,
    // driver-clickhouse) implements this trait and should be tested
    // independently with integration tests backed by real database instances.

    /// Verify that the trait contract exists and can be referenced.
    ///
    /// Implementation correctness is verified by integration tests in each
    /// driver crate. This test exists solely to confirm that the module
    /// structure compiles and the test harness is functional.
    #[test]
    fn database_driver_trait_has_no_pure_logic() {
        // The trait is a contract; its implementations are tested in their
        // respective crates.
    }
}

#[cfg(test)]
mod dialect_tests {
    use super::{quote_ident_backtick, quote_ident_double};

    #[test]
    fn double_quote_escapes_inner_quotes() {
        assert_eq!(quote_ident_double("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn backtick_escapes_inner_backticks() {
        assert_eq!(quote_ident_backtick("a`b"), "`a``b`");
    }
}
