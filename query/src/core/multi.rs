//! Multi-statement execution planner.
//!
//! Given a list of `Statement`s, produces a `BatchPlan` describing the
//! execution shape:
//!
//! - `begin_transaction` (true if the batch should wrap writes in a server-side
//!   `BEGIN ... COMMIT` block, false for ClickHouse or single-statement batches),
//! - per-statement metadata (already parsed by the splitter),
//! - whether to commit or rollback on success/failure.
//!
//! The actual execution is performed by callers (typically the UI layer),
//! which already has a live `DatabaseConnection` and can call
//! `execute_query_page` for each statement. This module is pure: it does
//! not touch IO, drivers, or the database.
//!
//! ## Semantics
//!
//! - **PG / MySQL / SQLite**: when a batch has ≥1 write statement, it is
//!   wrapped in `BEGIN; <stmts>; COMMIT;` (or `ROLLBACK;` on first error).
//!   Read-only batches are executed as-is.
//! - **ClickHouse**: no cross-statement transactions. Each statement runs
//!   individually; on first error, the batch stops and remaining statements
//!   are marked `Skipped`.
//! - A statement that returns `AffectedRows` is treated as a write for
//!   the purpose of `has_writes` detection.
//!
//! Caller is responsible for:
//! 1. Sending the `BEGIN` / `COMMIT` / `ROLLBACK` over the same connection
//!    when `needs_transaction()` is true.
//! 2. Executing each statement via `execute_query_page`.
//! 3. Updating the `StatementOutcome` for each statement.

use crate::core::splitter::{split_sql, Statement, StatementKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseFamily {
    Sqlite,
    Postgres,
    MySql,
    ClickHouse,
}

impl DatabaseFamily {
    /// Whether this family supports `BEGIN; ... COMMIT;` server-side
    /// transactions that wrap multiple write statements.
    pub fn supports_cross_statement_tx(self) -> bool {
        !matches!(self, DatabaseFamily::ClickHouse)
    }
}

impl From<models::DatabaseKind> for DatabaseFamily {
    fn from(kind: models::DatabaseKind) -> Self {
        match kind {
            models::DatabaseKind::Sqlite => DatabaseFamily::Sqlite,
            models::DatabaseKind::Postgres => DatabaseFamily::Postgres,
            models::DatabaseKind::MySql => DatabaseFamily::MySql,
            models::DatabaseKind::ClickHouse => DatabaseFamily::ClickHouse,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitKind {
    /// Send `BEGIN` before the first write, `COMMIT` at the end.
    Commit,
    /// Send `BEGIN` before the first write, `ROLLBACK` at the end (failure).
    Rollback,
    /// Read-only batch, no transaction needed.
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchPlan {
    pub family: DatabaseFamily,
    pub statements: Vec<Statement>,
    /// Per-statement expected outcome (caller can use to render progress).
    pub commit_kind: CommitKind,
    /// Total number of non-empty statements (skips Empty).
    pub executable_count: usize,
    /// True if any statement is `StatementKind::Write` (DDL/DML).
    pub has_writes: bool,
}

impl BatchPlan {
    /// True iff this plan should wrap writes in `BEGIN; ... COMMIT;`.
    pub fn needs_transaction(&self) -> bool {
        matches!(self.commit_kind, CommitKind::Commit | CommitKind::Rollback)
            && self.family.supports_cross_statement_tx()
    }
}

/// Build a `BatchPlan` for `sql` running against `family`.
pub fn plan_batch(sql: &str, family: DatabaseFamily) -> BatchPlan {
    let statements = split_sql(sql);
    let mut executable_count = 0;
    let mut has_writes = false;
    for stmt in &statements {
        if stmt.is_empty() {
            continue;
        }
        executable_count += 1;
        if matches!(stmt.kind, StatementKind::Write) {
            has_writes = true;
        }
    }

    let commit_kind = match (family.supports_cross_statement_tx(), has_writes) {
        (true, true) => CommitKind::Commit,
        (true, false) => CommitKind::None,
        // ClickHouse: never tx. Per-statement commit.
        (false, _) => CommitKind::None,
    };

    BatchPlan {
        family,
        statements,
        commit_kind,
        executable_count,
        has_writes,
    }
}

/// How the runner should treat a single statement (used by UI for
/// progress rendering and history grouping).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementOutcome {
    /// Statement executed successfully, returned some rows or affected rows.
    Ok { duration_ms: u64, rows: Option<usize> },
    /// Statement execution failed with this error message.
    Error { message: String },
    /// Statement was not executed because a previous statement in the
    /// batch failed and the runner short-circuits.
    Skipped,
}

impl StatementOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, StatementOutcome::Ok { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_single_select_no_transaction() {
        let plan = plan_batch("SELECT 1", DatabaseFamily::Postgres);
        assert_eq!(plan.statements.len(), 1);
        assert!(!plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::None);
        assert_eq!(plan.executable_count, 1);
        assert!(!plan.needs_transaction());
    }

    #[test]
    fn plan_mixed_batch_wraps_in_commit() {
        let plan = plan_batch(
            "INSERT INTO t VALUES (1); SELECT * FROM t; UPDATE t SET x = 1;",
            DatabaseFamily::Postgres,
        );
        assert_eq!(plan.statements.len(), 3);
        assert!(plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::Commit);
        assert!(plan.needs_transaction());
        assert_eq!(plan.executable_count, 3);
    }

    #[test]
    fn plan_clickhouse_never_wraps() {
        let plan = plan_batch(
            "INSERT INTO t VALUES (1); SELECT * FROM t;",
            DatabaseFamily::ClickHouse,
        );
        assert!(plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::None);
        assert!(!plan.needs_transaction());
    }

    #[test]
    fn plan_with_only_comments_executable_count_zero() {
        let plan = plan_batch(
            "-- just a comment\n-- another one\n",
            DatabaseFamily::Postgres,
        );
        assert_eq!(plan.executable_count, 0);
        assert!(!plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::None);
    }

    #[test]
    fn plan_with_dollar_quote_body_stays_one_statement() {
        let plan = plan_batch(
            "CREATE FUNCTION f() RETURNS void AS $$ BEGIN SELECT 1; END; $$ LANGUAGE plpgsql;",
            DatabaseFamily::Postgres,
        );
        // `;` inside the dollar-quote body should not split the statement.
        assert_eq!(plan.statements.len(), 1);
        assert!(plan.has_writes);
    }

    #[test]
    fn plan_sqlite_write_batch() {
        let plan = plan_batch(
            "INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);",
            DatabaseFamily::Sqlite,
        );
        assert!(plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::Commit);
        assert!(plan.needs_transaction());
    }

    #[test]
    fn plan_mysql_read_only() {
        let plan = plan_batch("SELECT 1; SELECT 2;", DatabaseFamily::MySql);
        assert!(!plan.has_writes);
        assert_eq!(plan.commit_kind, CommitKind::None);
        assert!(!plan.needs_transaction());
    }

    #[test]
    fn statement_outcome_helpers() {
        let ok = StatementOutcome::Ok {
            duration_ms: 12,
            rows: Some(5),
        };
        let err = StatementOutcome::Error {
            message: "boom".into(),
        };
        let sk = StatementOutcome::Skipped;
        assert!(ok.is_ok());
        assert!(!err.is_ok());
        assert!(!sk.is_ok());
    }
}
