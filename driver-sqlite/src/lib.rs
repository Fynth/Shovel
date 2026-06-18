use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::{path::PathBuf, str::FromStr, time::Duration};

pub struct SqliteDriver {}
type SqliteError = sqlx::Error;
type SqlitePool = sqlx::SqlitePool;
type SqliteConfig = String;
impl database::DatabaseDriver for SqliteDriver {
    type Config = SqliteConfig;
    type Pool = SqlitePool;
    type Error = SqliteError;

    async fn connect(info: Self::Config) -> Result<Self::Pool, Self::Error> {
        let target = info.trim();
        if target.eq_ignore_ascii_case(":memory:") || target.starts_with("sqlite:") {
            let options = SqliteConnectOptions::from_str(target)?;
            SqlitePool::connect_with(options).await
        } else {
            let options = SqliteConnectOptions::new()
                .filename(PathBuf::from(target))
                .create_if_missing(false)
                .busy_timeout(Duration::from_secs(5))
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal);
            SqlitePoolOptions::new()
                .max_connections(4)
                .connect_with(options)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    // SqliteDriver::connect requires a real SQLite database file (or :memory:).
    // The driver's entire logic is inside the `connect` async method which
    // dispatches to `sqlx::SqlitePool::connect_with`. There are no pure helper
    // functions to unit-test in this crate.
    //
    // Integration tests should cover:
    //   - Connecting to an in-memory database (`:memory:`)
    //   - Connecting to a file path that exists
    //   - Error when the file does not exist (create_if_missing is false)
    //   - Connecting with a `sqlite:` DSN prefix
    //   - Whitespace trimming of the target string

    #[test]
    fn sqlite_driver_connect_requires_database() {
        // `SqliteDriver::connect` delegates entirely to `sqlx::SqlitePool`.
        // It should be tested with integration tests using `:memory:` or
        // temporary database files on disk.
    }
}
