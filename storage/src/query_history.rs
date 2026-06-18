use models::QueryHistoryItem;
use sqlx::SqlitePool;

use crate::fs_store::query_history_path;

const MAX_HISTORY_ITEMS: usize = 20;

/// SQLite-backed storage for query history with FTS5 search support.
pub struct QueryHistoryStore;

impl QueryHistoryStore {
    /// Initialize the store, creating tables and migrating from JSON if needed.
    pub async fn init() -> Result<(), String> {
        let pool = crate::chat::chat_pool().await?;
        initialize_schema(pool).await?;
        migrate_from_json(pool).await?;
        Ok(())
    }

    /// Save a query history item.
    pub async fn save(item: &QueryHistoryItem) -> Result<(), String> {
        let pool = crate::chat::chat_pool().await?;

        sqlx::query(
            r#"
            INSERT INTO query_history (
                id, sql, duration_ms, rows_returned, executed_at,
                connection_name, connection_type, outcome, error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(item.id.to_string())
        .bind(&item.sql)
        .bind(item.duration_ms as i64)
        .bind(item.rows_returned.map(|r| r as i64))
        .bind(item.executed_at)
        .bind(&item.connection_name)
        .bind(&item.connection_type)
        .bind(&item.outcome)
        .bind(item.error_message.as_ref())
        .execute(pool)
        .await
        .map_err(|err| format!("failed to save query history: {err}"))?;

        // Insert into FTS5 index
        sqlx::query(
            r#"
            INSERT INTO query_history_fts (rowid, sql, connection_name)
            VALUES (last_insert_rowid(), ?, ?)
            "#,
        )
        .bind(&item.sql)
        .bind(&item.connection_name)
        .execute(pool)
        .await
        .map_err(|err| format!("failed to index query history: {err}"))?;

        // Trim to max items
        trim_to_max(pool, MAX_HISTORY_ITEMS).await?;

        Ok(())
    }

    /// Load query history with optional limit.
    pub async fn load(limit: usize) -> Result<Vec<QueryHistoryItem>, String> {
        let pool = crate::chat::chat_pool().await?;

        let rows = sqlx::query(
            r#"
            SELECT
                id, sql, duration_ms, rows_returned, executed_at,
                connection_name, connection_type, outcome, error_message
            FROM query_history
            ORDER BY executed_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(pool)
        .await
        .map_err(|err| format!("failed to load query history: {err}"))?;

        rows.into_iter().map(row_to_item).collect()
    }

    /// Search query history using FTS5.
    pub async fn search(query: &str) -> Result<Vec<QueryHistoryItem>, String> {
        let pool = crate::chat::chat_pool().await?;

        // Use FTS5 to find matching rows
        let rows = sqlx::query(
            r#"
            SELECT
                h.id, h.sql, h.duration_ms, h.rows_returned, h.executed_at,
                h.connection_name, h.connection_type, h.outcome, h.error_message
            FROM query_history h
            JOIN query_history_fts f ON h.rowid = f.rowid
            WHERE query_history_fts MATCH ?
            ORDER BY h.executed_at DESC
            LIMIT 50
            "#,
        )
        .bind(query)
        .fetch_all(pool)
        .await
        .map_err(|err| format!("failed to search query history: {err}"))?;

        rows.into_iter().map(row_to_item).collect()
    }

    /// Get the total count of history items.
    pub async fn count() -> Result<i64, String> {
        let pool = crate::chat::chat_pool().await?;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM query_history")
            .fetch_one(pool)
            .await
            .map_err(|err| format!("failed to count query history: {err}"))?;

        Ok(count)
    }

    /// Delete a single history entry by its `id`.
    ///
    /// Returns `true` when a row was removed, `false` when the id
    /// was not present (idempotent — safe to call from a UI without
    /// pre-checking existence).
    ///
    /// The matching FTS5 row is removed in the same call so the
    /// search index never references a stale `rowid`.
    pub async fn delete(id: u64) -> Result<bool, String> {
        let pool = crate::chat::chat_pool().await?;

        // Drop the FTS5 row first. content_rowid points at the
        // main table's rowid, so look it up explicitly.
        sqlx::query(
            r#"
            DELETE FROM query_history_fts
            WHERE rowid IN (SELECT rowid FROM query_history WHERE id = ?)
            "#,
        )
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(|err| format!("failed to remove FTS5 row for history {id}: {err}"))?;

        let result = sqlx::query("DELETE FROM query_history WHERE id = ?")
            .bind(id.to_string())
            .execute(pool)
            .await
            .map_err(|err| format!("failed to delete query history {id}: {err}"))?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete every history entry.
    ///
    /// Used by the "Clear all history" menu item. Both the main
    /// table and the FTS5 index are wiped so search results stay
    /// consistent.
    pub async fn clear() -> Result<u64, String> {
        let pool = crate::chat::chat_pool().await?;

        // FTS5 first — even though the main table references the
        // FTS rowids by content_rowid, dropping the FTS table while
        // main rows still exist would leave orphan search terms.
        sqlx::query("DELETE FROM query_history_fts")
            .execute(pool)
            .await
            .map_err(|err| format!("failed to clear query history FTS index: {err}"))?;

        let result = sqlx::query("DELETE FROM query_history")
            .execute(pool)
            .await
            .map_err(|err| format!("failed to clear query history: {err}"))?;

        Ok(result.rows_affected())
    }
}

async fn initialize_schema(pool: &SqlitePool) -> Result<(), String> {
    // Main query_history table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS query_history (
            id TEXT PRIMARY KEY,
            sql TEXT NOT NULL,
            duration_ms INTEGER,
            rows_returned INTEGER,
            executed_at INTEGER NOT NULL,
            connection_name TEXT,
            connection_type TEXT,
            outcome TEXT NOT NULL,
            error_message TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create query_history table: {err}"))?;

    // FTS5 virtual table for full-text search
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS query_history_fts USING fts5(
            sql, connection_name,
            content_rowid=rowid
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create FTS5 table: {err}"))?;

    // Indexes for performance
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_query_history_executed_at ON query_history(executed_at)",
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create executed_at index: {err}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_query_history_connection ON query_history(connection_name)",
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create connection index: {err}"))?;

    Ok(())
}

async fn migrate_from_json(pool: &SqlitePool) -> Result<(), String> {
    let json_path = query_history_path();

    // Check if JSON file exists
    if !json_path.exists() {
        return Ok(());
    }

    // Read JSON file
    let content = match tokio::fs::read_to_string(&json_path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("failed to read query history JSON: {err}")),
    };

    if content.trim().is_empty() {
        return Ok(());
    }

    // Parse JSON
    let items: Vec<QueryHistoryItem> = match serde_json::from_str(&content) {
        Ok(items) => items,
        Err(err) => {
            eprintln!("Warning: Failed to parse query_history.json: {err}");
            return Ok(());
        }
    };

    if items.is_empty() {
        return Ok(());
    }

    // Insert items into SQLite
    for item in &items {
        // Use INSERT OR IGNORE to avoid duplicates
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO query_history (
                id, sql, duration_ms, rows_returned, executed_at,
                connection_name, connection_type, outcome, error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(item.id.to_string())
        .bind(&item.sql)
        .bind(item.duration_ms as i64)
        .bind(item.rows_returned.map(|r| r as i64))
        .bind(item.executed_at)
        .bind(&item.connection_name)
        .bind(&item.connection_type)
        .bind(&item.outcome)
        .bind(item.error_message.as_ref())
        .execute(pool)
        .await;

        if let Err(err) = result {
            eprintln!("Warning: Failed to migrate history item {}: {err}", item.id);
            continue;
        }

        // Also add to FTS5 index
        let _ = sqlx::query(
            r#"
            INSERT INTO query_history_fts (rowid, sql, connection_name)
            SELECT rowid, ?, ? FROM query_history WHERE id = ?
            "#,
        )
        .bind(&item.sql)
        .bind(&item.connection_name)
        .bind(item.id.to_string())
        .execute(pool)
        .await;
    }

    // Rename JSON file to .backup
    let backup_path = json_path.with_extension("json.backup");
    if let Err(err) = tokio::fs::rename(&json_path, &backup_path).await {
        eprintln!("Warning: Failed to rename query_history.json to backup: {err}");
    } else {
        println!(
            "Migrated {} query history items from JSON to SQLite. Backup: {}",
            items.len(),
            backup_path.display()
        );
    }

    Ok(())
}

async fn trim_to_max(pool: &SqlitePool, max: usize) -> Result<(), String> {
    // Get IDs of items to delete
    let ids_to_delete: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT id FROM query_history
        ORDER BY executed_at DESC
        LIMIT -1 OFFSET ?
        "#,
    )
    .bind(max as i64)
    .fetch_all(pool)
    .await
    .map_err(|err| format!("failed to get old history items: {err}"))?;

    if ids_to_delete.is_empty() {
        return Ok(());
    }

    // Delete from FTS5 index first
    for id in &ids_to_delete {
        let _ = sqlx::query(
            r#"
            DELETE FROM query_history_fts
            WHERE rowid IN (SELECT rowid FROM query_history WHERE id = ?)
            "#,
        )
        .bind(id)
        .execute(pool)
        .await;
    }

    // Delete from main table
    let placeholders = ids_to_delete
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("DELETE FROM query_history WHERE id IN ({})", placeholders);

    let mut query_builder = sqlx::query(&query);
    for id in &ids_to_delete {
        query_builder = query_builder.bind(id);
    }

    query_builder
        .execute(pool)
        .await
        .map_err(|err| format!("failed to trim query history: {err}"))?;

    Ok(())
}

fn row_to_item(row: sqlx::sqlite::SqliteRow) -> Result<QueryHistoryItem, String> {
    use sqlx::Row;

    let id_str: String = row.try_get("id").map_err(|e| e.to_string())?;
    let id: u64 = id_str.parse().map_err(|e| format!("invalid id: {e}"))?;

    Ok(QueryHistoryItem {
        id,
        sql: row.try_get("sql").map_err(|e| e.to_string())?,
        duration_ms: row.try_get::<i64, _>("duration_ms").unwrap_or(0) as u64,
        rows_returned: row
            .try_get::<Option<i64>, _>("rows_returned")
            .ok()
            .flatten()
            .map(|r| r as usize),
        executed_at: row.try_get("executed_at").map_err(|e| e.to_string())?,
        connection_name: row.try_get("connection_name").unwrap_or_default(),
        connection_type: row.try_get("connection_type").unwrap_or_default(),
        outcome: row.try_get("outcome").unwrap_or_default(),
        error_message: row.try_get("error_message").ok(),
        tab_title: String::new(), // Not stored in DB
    })
}
