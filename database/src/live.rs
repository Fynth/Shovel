use models::{ClickHouseFormData, DatabaseKind};

#[derive(Clone, Debug)]
pub enum LiveConnection {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
    MySql(sqlx::MySqlPool),
    ClickHouse(ClickHouseFormData),
}

impl LiveConnection {
    /// Returns the [`DatabaseKind`] for this connection without inspecting the pool.
    pub fn kind(&self) -> DatabaseKind {
        match self {
            LiveConnection::Sqlite(_) => DatabaseKind::Sqlite,
            LiveConnection::Postgres(_) => DatabaseKind::Postgres,
            LiveConnection::MySql(_) => DatabaseKind::MySql,
            LiveConnection::ClickHouse(_) => DatabaseKind::ClickHouse,
        }
    }

    /// Returns `true` if this is a SQLite connection.
    pub fn is_sqlite(&self) -> bool {
        matches!(self, LiveConnection::Sqlite(_))
    }

    /// Returns `true` if this is a PostgreSQL connection.
    pub fn is_postgres(&self) -> bool {
        matches!(self, LiveConnection::Postgres(_))
    }

    /// Returns `true` if this is a MySQL connection.
    pub fn is_mysql(&self) -> bool {
        matches!(self, LiveConnection::MySql(_))
    }

    /// Returns `true` if this is a ClickHouse connection.
    pub fn is_clickhouse(&self) -> bool {
        matches!(self, LiveConnection::ClickHouse(_))
    }

    /// Returns the human-facing name of the database kind (e.g. "SQLite", "PostgreSQL").
    pub fn kind_name(&self) -> &'static str {
        self.kind().display_name()
    }
}
