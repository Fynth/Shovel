mod rows;
mod session;

pub use session::SqliteSession;

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
    use super::{SqliteDriver, SqliteSession};
    use database::{DatabaseDriver, SessionHandle};
    use models::QueryOutput;
    use std::sync::Arc;

    #[tokio::test]
    async fn sqlite_session_executes_select() {
        let pool = SqliteDriver::connect(":memory:".into()).await.unwrap();
        sqlx::query("create table items (id integer, name text)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("insert into items values (1, 'a')")
            .execute(&pool)
            .await
            .unwrap();
        let handle = SessionHandle::wrap(Arc::new(SqliteSession { pool }));
        let out = handle
            .query()
            .execute_sql("select id, name from items")
            .await
            .unwrap();
        match out {
            QueryOutput::Table(page) => assert_eq!(page.rows.len(), 1),
            other => panic!("{other:?}"),
        }
    }
}
