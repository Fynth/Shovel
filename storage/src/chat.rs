use std::time::{Duration, SystemTime, UNIX_EPOCH};

use models::{AcpMessageKind, AcpUiMessage, ChatArtifact, ChatThreadSummary};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use tokio::{fs, sync::OnceCell};

use crate::fs_store::chat_db_path;

static CHAT_POOL: OnceCell<SqlitePool> = OnceCell::const_new();
static VEC_EXTENSION_INITIALIZED: std::sync::Once = std::sync::Once::new();

/// Ensure sqlite-vec extension is registered as an auto-extension.
/// This must be called before creating any SQLite connections that need vec0 support.
pub fn ensure_vec_extension_initialized() {
    VEC_EXTENSION_INITIALIZED.call_once(|| unsafe {
        libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut libsqlite3_sys::sqlite3,
                *mut *mut i8,
                *const libsqlite3_sys::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

/// Load all chat threads, ordered by most recently updated first.
///
/// Each returned [`ChatThreadSummary`] includes the thread metadata plus a
/// preview of the last message in the thread.
///
/// # Errors
///
/// Returns an error string if the database query fails.
pub async fn load_chat_threads() -> Result<Vec<ChatThreadSummary>, String> {
    let pool = chat_pool().await?;
    let rows = sqlx::query(
        r#"
        select
          t.id,
          t.title,
          t.connection_name,
          t.created_at,
          t.updated_at,
          coalesce(
            (
              select m.text
              from chat_messages m
              where m.thread_id = t.id
              order by m.position desc, m.id desc
              limit 1
            ),
            ''
          ) as last_message_preview
        from chat_threads t
        order by t.updated_at desc, t.id desc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|err| format!("failed to load chat threads: {err}"))?;

    rows.into_iter().map(row_to_thread_summary).collect()
}

/// Create a new chat thread and return its summary.
///
/// If `title` is `None` or empty, the title defaults to `"New chat"`.
/// The `connection_name` is normalized before storage.
///
/// # Arguments
///
/// * `connection_name` - The name of the database connection this thread is associated with.
/// * `title` - An optional title for the thread.
///
/// # Errors
///
/// Returns an error string if the database insert fails.
pub async fn create_chat_thread(
    connection_name: String,
    title: Option<String>,
) -> Result<ChatThreadSummary, String> {
    let pool = chat_pool().await?;
    let now = unix_timestamp();
    let title = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "New chat".to_string());
    let connection_name = normalized_connection_name(&connection_name);

    let result = sqlx::query(
        r#"
        insert into chat_threads (title, connection_name, created_at, updated_at)
        values (?1, ?2, ?3, ?4)
        "#,
    )
    .bind(&title)
    .bind(&connection_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create chat thread: {err}"))?;

    Ok(ChatThreadSummary {
        id: result.last_insert_rowid(),
        title,
        connection_name,
        created_at: now,
        updated_at: now,
        last_message_preview: String::new(),
    })
}

/// Delete a chat thread and all its messages.
///
/// This is a cascading delete: removing the thread row also removes
/// associated messages thanks to the foreign-key schema.
///
/// # Arguments
///
/// * `thread_id` - The database ID of the thread to delete.
///
/// # Errors
///
/// Returns an error string if the database delete fails.
pub async fn delete_chat_thread(thread_id: i64) -> Result<(), String> {
    let pool = chat_pool().await?;
    sqlx::query("delete from chat_threads where id = ?1")
        .bind(thread_id)
        .execute(pool)
        .await
        .map_err(|err| format!("failed to delete chat thread {thread_id}: {err}"))?;
    Ok(())
}

/// Load all messages for a chat thread, ordered by position.
///
/// # Arguments
///
/// * `thread_id` - The database ID of the thread whose messages to load.
///
/// # Returns
///
/// A vector of [`AcpUiMessage`] values in positional order, or an error string
/// if the database query fails.
pub async fn load_chat_thread_messages(thread_id: i64) -> Result<Vec<AcpUiMessage>, String> {
    let pool = chat_pool().await?;
    let rows = sqlx::query(
        r#"
        select id, kind, text, created_at, artifact_json
        from chat_messages
        where thread_id = ?1
        order by position asc, id asc
        "#,
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(|err| format!("failed to load chat thread {thread_id}: {err}"))?;

    rows.into_iter().map(row_to_message).collect()
}

/// Atomically replace all messages in a chat thread and update its metadata.
///
/// This runs inside a single SQLite transaction: existing messages are deleted,
/// new messages are inserted with sequential positions, and the thread's title,
/// connection name, and `updated_at` timestamp are refreshed.
///
/// # Arguments
///
/// * `thread_id` - The database ID of the thread to update.
/// * `title` - The new title for the thread (normalized before storage).
/// * `connection_name` - The connection name to associate (normalized before storage).
/// * `messages` - The complete set of messages to store, in display order.
///
/// # Returns
///
/// A [`ChatThreadSummary`] reflecting the updated thread state.
///
/// # Errors
///
/// Returns an error string if any database operation fails or the transaction
/// cannot be committed.
pub async fn save_chat_thread_snapshot(
    thread_id: i64,
    title: String,
    connection_name: String,
    messages: Vec<AcpUiMessage>,
) -> Result<ChatThreadSummary, String> {
    let pool = chat_pool().await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|err| format!("failed to start chat transaction: {err}"))?;
    let now = unix_timestamp();
    let title = normalized_thread_title(&title);
    let connection_name = normalized_connection_name(&connection_name);

    sqlx::query(
        r#"
        update chat_threads
        set title = ?1, connection_name = ?2, updated_at = ?3
        where id = ?4
        "#,
    )
    .bind(&title)
    .bind(&connection_name)
    .bind(now)
    .bind(thread_id)
    .execute(&mut *tx)
    .await
    .map_err(|err| format!("failed to update chat thread {thread_id}: {err}"))?;

    sqlx::query("delete from chat_messages where thread_id = ?1")
        .bind(thread_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| format!("failed to replace chat thread {thread_id}: {err}"))?;

    for (position, message) in messages.iter().enumerate() {
        let artifact_json = message
            .artifact
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| format!("failed to serialize chat artifact: {err}"))?;

        sqlx::query(
            r#"
            insert into chat_messages (
              thread_id,
              position,
              kind,
              text,
              created_at,
              artifact_json
            )
            values (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(thread_id)
        .bind(position as i64)
        .bind(message_kind_to_db(&message.kind))
        .bind(&message.text)
        .bind(message.created_at)
        .bind(artifact_json)
        .execute(&mut *tx)
        .await
        .map_err(|err| format!("failed to store chat message: {err}"))?;
    }

    tx.commit()
        .await
        .map_err(|err| format!("failed to commit chat transaction: {err}"))?;

    let created_at =
        sqlx::query_scalar::<_, i64>("select created_at from chat_threads where id = ?1 limit 1")
            .bind(thread_id)
            .fetch_one(pool)
            .await
            .map_err(|err| format!("failed to reload chat thread {thread_id}: {err}"))?;

    Ok(ChatThreadSummary {
        id: thread_id,
        title,
        connection_name,
        created_at,
        updated_at: now,
        last_message_preview: compact_preview(
            messages
                .last()
                .map(|message| message.text.as_str())
                .unwrap_or_default(),
        ),
    })
}

/// Full-text search across chat messages using FTS5.
///
/// Returns distinct threads whose messages match the FTS5 query string.
/// Results are ordered by most recently updated thread first.
///
/// # Arguments
///
/// * `query` - An FTS5 match expression (e.g. `"SELECT"` or `"schema AND table"`).
/// * `limit` - Maximum number of thread summaries to return.
///
/// # Errors
///
/// Returns an error string if the FTS5 query fails.
pub async fn search_chat_messages(
    query: &str,
    limit: usize,
) -> Result<Vec<ChatThreadSummary>, String> {
    let pool = chat_pool().await?;

    let rows = sqlx::query(
        r#"
        SELECT DISTINCT
            t.id,
            t.title,
            t.connection_name,
            t.created_at,
            t.updated_at,
            coalesce(
                (
                    select m.text
                    from chat_messages m
                    where m.thread_id = t.id
                    order by m.position desc, m.id desc
                    limit 1
                ),
                ''
            ) as last_message_preview
        FROM chat_threads t
        JOIN chat_messages_fts f ON t.id = f.thread_id
        WHERE chat_messages_fts MATCH ?
        ORDER BY t.updated_at DESC
        LIMIT ?
        "#,
    )
    .bind(query)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .map_err(|err| format!("failed to search chat messages: {err}"))?;

    rows.into_iter().map(row_to_thread_summary).collect()
}

/// Find chat threads that contain SQL draft artifacts.
///
/// Scans messages of kind `tool` whose serialized artifact JSON contains
/// `"SqlDraft"`, returning the owning threads ordered by most recently
/// updated first.
///
/// # Arguments
///
/// * `limit` - Maximum number of thread summaries to return.
///
/// # Errors
///
/// Returns an error string if the database query fails.
pub async fn search_chat_sql_artifacts(limit: usize) -> Result<Vec<ChatThreadSummary>, String> {
    let pool = chat_pool().await?;

    let rows = sqlx::query(
        r#"
        SELECT DISTINCT
            t.id,
            t.title,
            t.connection_name,
            t.created_at,
            t.updated_at,
            coalesce(
                (
                    select m.text
                    from chat_messages m
                    where m.thread_id = t.id
                    order by m.position desc, m.id desc
                    limit 1
                ),
                ''
            ) as last_message_preview
        FROM chat_threads t
        JOIN chat_messages m ON t.id = m.thread_id
        WHERE m.kind = 'tool' AND m.artifact_json LIKE '%SqlDraft%'
        ORDER BY t.updated_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .map_err(|err| format!("failed to search SQL artifacts: {err}"))?;

    rows.into_iter().map(row_to_thread_summary).collect()
}

pub(crate) async fn chat_pool() -> Result<&'static SqlitePool, String> {
    CHAT_POOL
        .get_or_try_init(|| async {
            ensure_vec_extension_initialized();

            let path = chat_db_path();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|err| format!("failed to create chat storage dir: {err}"))?;
            }

            let options = SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5))
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal);
            let pool = SqlitePool::connect_with(options)
                .await
                .map_err(|err| format!("failed to open chat database {}: {err}", path.display()))?;

            initialize_schema(&pool).await?;
            Ok(pool)
        })
        .await
}

async fn initialize_schema(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        create table if not exists chat_threads (
          id integer primary key autoincrement,
          title text not null,
          connection_name text not null,
          created_at integer not null,
          updated_at integer not null
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to initialize chat thread schema: {err}"))?;

    sqlx::query(
        r#"
        create table if not exists chat_messages (
          id integer primary key autoincrement,
          thread_id integer not null,
          position integer not null,
          kind text not null,
          text text not null,
          created_at integer not null,
          artifact_json text,
          foreign key(thread_id) references chat_threads(id) on delete cascade
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to initialize chat message schema: {err}"))?;

    sqlx::query(
        "create index if not exists idx_chat_threads_updated_at on chat_threads(updated_at desc)",
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to initialize chat thread index: {err}"))?;

    sqlx::query(
        "create unique index if not exists idx_chat_messages_thread_position on chat_messages(thread_id, position)",
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to initialize chat message index: {err}"))?;

    // FTS5 virtual table for full-text search on chat messages
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS chat_messages_fts USING fts5(
            thread_id UNINDEXED,
            position UNINDEXED,
            content
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create chat_messages_fts table: {err}"))?;

    recreate_chat_fts_triggers(pool).await?;
    rebuild_chat_fts_index(pool).await?;

    Ok(())
}

async fn recreate_chat_fts_triggers(pool: &SqlitePool) -> Result<(), String> {
    for trigger_name in ["chat_messages_ai", "chat_messages_au", "chat_messages_ad"] {
        sqlx::query(&format!("drop trigger if exists {trigger_name}"))
            .execute(pool)
            .await
            .map_err(|err| format!("failed to drop chat_messages trigger {trigger_name}: {err}"))?;
    }

    sqlx::query(
        r#"
        CREATE TRIGGER chat_messages_ai AFTER INSERT ON chat_messages BEGIN
            INSERT INTO chat_messages_fts (thread_id, position, content)
            VALUES (new.thread_id, new.position, new.text);
        END
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create chat_messages insert trigger: {err}"))?;

    sqlx::query(
        r#"
        CREATE TRIGGER chat_messages_au AFTER UPDATE ON chat_messages BEGIN
            DELETE FROM chat_messages_fts
            WHERE thread_id = old.thread_id AND position = old.position;
            INSERT INTO chat_messages_fts (thread_id, position, content)
            VALUES (new.thread_id, new.position, new.text);
        END
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create chat_messages update trigger: {err}"))?;

    sqlx::query(
        r#"
        CREATE TRIGGER chat_messages_ad AFTER DELETE ON chat_messages BEGIN
            DELETE FROM chat_messages_fts
            WHERE thread_id = old.thread_id AND position = old.position;
        END
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to create chat_messages delete trigger: {err}"))?;

    Ok(())
}

async fn rebuild_chat_fts_index(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("delete from chat_messages_fts")
        .execute(pool)
        .await
        .map_err(|err| format!("failed to clear chat_messages_fts index: {err}"))?;

    sqlx::query(
        r#"
        insert into chat_messages_fts (thread_id, position, content)
        select thread_id, position, text
        from chat_messages
        order by thread_id asc, position asc, id asc
        "#,
    )
    .execute(pool)
    .await
    .map_err(|err| format!("failed to rebuild chat_messages_fts index: {err}"))?;

    Ok(())
}

fn row_to_thread_summary(row: sqlx::sqlite::SqliteRow) -> Result<ChatThreadSummary, String> {
    Ok(ChatThreadSummary {
        id: row.try_get("id").map_err(db_read_error)?,
        title: row.try_get("title").map_err(db_read_error)?,
        connection_name: row.try_get("connection_name").map_err(db_read_error)?,
        created_at: row.try_get("created_at").map_err(db_read_error)?,
        updated_at: row.try_get("updated_at").map_err(db_read_error)?,
        last_message_preview: compact_preview(
            &row.try_get::<String, _>("last_message_preview")
                .map_err(db_read_error)?,
        ),
    })
}

fn row_to_message(row: sqlx::sqlite::SqliteRow) -> Result<AcpUiMessage, String> {
    let kind_raw = row.try_get::<String, _>("kind").map_err(db_read_error)?;
    let artifact_json = row
        .try_get::<Option<String>, _>("artifact_json")
        .map_err(db_read_error)?;

    let artifact = match artifact_json {
        Some(json) if !json.trim().is_empty() => Some(
            serde_json::from_str::<ChatArtifact>(&json)
                .map_err(|err| format!("failed to parse stored chat artifact: {err}"))?,
        ),
        _ => None,
    };

    Ok(AcpUiMessage {
        id: row.try_get::<i64, _>("id").map_err(db_read_error)? as u64,
        kind: db_to_message_kind(&kind_raw)?,
        text: row.try_get("text").map_err(db_read_error)?,
        created_at: row.try_get("created_at").map_err(db_read_error)?,
        artifact,
    })
}

fn message_kind_to_db(kind: &AcpMessageKind) -> &'static str {
    match kind {
        AcpMessageKind::User => "user",
        AcpMessageKind::Agent => "agent",
        AcpMessageKind::Thought => "thought",
        AcpMessageKind::Tool => "tool",
        AcpMessageKind::System => "system",
        AcpMessageKind::Error => "error",
    }
}

fn db_to_message_kind(value: &str) -> Result<AcpMessageKind, String> {
    match value {
        "user" => Ok(AcpMessageKind::User),
        "agent" => Ok(AcpMessageKind::Agent),
        "thought" => Ok(AcpMessageKind::Thought),
        "tool" => Ok(AcpMessageKind::Tool),
        "system" => Ok(AcpMessageKind::System),
        "error" => Ok(AcpMessageKind::Error),
        other => Err(format!("unsupported stored chat message kind `{other}`")),
    }
}

fn db_read_error(err: sqlx::Error) -> String {
    format!("failed to read chat row: {err}")
}

fn normalized_thread_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "New chat".to_string()
    } else {
        title.to_string()
    }
}

fn normalized_connection_name(connection_name: &str) -> String {
    let connection_name = connection_name.trim();
    if connection_name.is_empty() {
        "No connection".to_string()
    } else {
        connection_name.to_string()
    }
}

fn compact_preview(text: &str) -> String {
    let preview = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_PREVIEW_CHARS: usize = 100;

    if preview.chars().count() <= MAX_PREVIEW_CHARS {
        preview
    } else {
        let truncated = preview.chars().take(MAX_PREVIEW_CHARS).collect::<String>();
        format!("{truncated}...")
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to create test pool")
    }

    #[tokio::test]
    async fn initialize_schema_migrates_legacy_fts_delete_triggers() {
        let pool = create_test_pool().await;

        sqlx::query(
            r#"
            create table chat_threads (
              id integer primary key autoincrement,
              title text not null,
              connection_name text not null,
              created_at integer not null,
              updated_at integer not null
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create chat_threads");

        sqlx::query(
            r#"
            create table chat_messages (
              id integer primary key autoincrement,
              thread_id integer not null,
              position integer not null,
              kind text not null,
              text text not null,
              created_at integer not null,
              artifact_json text,
              foreign key(thread_id) references chat_threads(id) on delete cascade
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create chat_messages");

        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE chat_messages_fts USING fts5(
                thread_id UNINDEXED,
                position UNINDEXED,
                content
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create legacy chat_messages_fts");

        sqlx::query(
            r#"
            CREATE TRIGGER chat_messages_ai AFTER INSERT ON chat_messages BEGIN
                INSERT INTO chat_messages_fts (thread_id, position, content)
                VALUES (new.thread_id, new.position, new.text);
            END
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create legacy insert trigger");

        sqlx::query(
            r#"
            CREATE TRIGGER chat_messages_au AFTER UPDATE ON chat_messages BEGIN
                INSERT INTO chat_messages_fts (chat_messages_fts, thread_id, position, content)
                VALUES ('delete', old.thread_id, old.position, old.text);
                INSERT INTO chat_messages_fts (thread_id, position, content)
                VALUES (new.thread_id, new.position, new.text);
            END
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create legacy update trigger");

        sqlx::query(
            r#"
            CREATE TRIGGER chat_messages_ad AFTER DELETE ON chat_messages BEGIN
                INSERT INTO chat_messages_fts (chat_messages_fts, thread_id, position, content)
                VALUES ('delete', old.thread_id, old.position, old.text);
            END
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create legacy delete trigger");

        sqlx::query(
            r#"
            insert into chat_threads (id, title, connection_name, created_at, updated_at)
            values (1, 'Thread', 'No connection', 1, 1)
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to insert thread");

        sqlx::query(
            r#"
            insert into chat_messages (thread_id, position, kind, text, created_at)
            values (1, 0, 'user', '', 1)
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to insert message");

        let legacy_delete = sqlx::query("delete from chat_messages where thread_id = 1")
            .execute(&pool)
            .await;
        assert!(legacy_delete.is_err(), "legacy delete trigger should fail");

        initialize_schema(&pool)
            .await
            .expect("failed to migrate legacy chat schema");

        sqlx::query("delete from chat_messages where thread_id = 1")
            .execute(&pool)
            .await
            .expect("migrated delete trigger should succeed");

        let fts_count: i64 = sqlx::query_scalar("select count(*) from chat_messages_fts")
            .fetch_one(&pool)
            .await
            .expect("failed to count chat_messages_fts rows");
        assert_eq!(fts_count, 0);
    }

    #[tokio::test]
    async fn initialize_schema_backfills_fts_index_from_existing_messages() {
        let pool = create_test_pool().await;

        sqlx::query(
            r#"
            create table chat_threads (
              id integer primary key autoincrement,
              title text not null,
              connection_name text not null,
              created_at integer not null,
              updated_at integer not null
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create chat_threads");

        sqlx::query(
            r#"
            create table chat_messages (
              id integer primary key autoincrement,
              thread_id integer not null,
              position integer not null,
              kind text not null,
              text text not null,
              created_at integer not null,
              artifact_json text,
              foreign key(thread_id) references chat_threads(id) on delete cascade
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create chat_messages");

        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE chat_messages_fts USING fts5(
                thread_id UNINDEXED,
                position UNINDEXED,
                content
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to create chat_messages_fts");

        sqlx::query(
            r#"
            insert into chat_threads (id, title, connection_name, created_at, updated_at)
            values (1, 'Thread', 'No connection', 1, 1)
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to insert thread");

        sqlx::query(
            r#"
            insert into chat_messages (thread_id, position, kind, text, created_at)
            values (1, 0, 'user', 'hello schema migration', 1)
            "#,
        )
        .execute(&pool)
        .await
        .expect("failed to insert message");

        initialize_schema(&pool)
            .await
            .expect("failed to initialize schema");

        let fts_count: i64 = sqlx::query_scalar("select count(*) from chat_messages_fts")
            .fetch_one(&pool)
            .await
            .expect("failed to count chat_messages_fts rows");
        assert_eq!(fts_count, 1);

        let search_hits: i64 = sqlx::query_scalar(
            "select count(*) from chat_messages_fts where chat_messages_fts match 'hello'",
        )
        .fetch_one(&pool)
        .await
        .expect("failed to search chat_messages_fts");
        assert_eq!(search_hits, 1);
    }
}
