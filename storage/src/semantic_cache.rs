use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chat::chat_pool;

/// Semantic cache entry ID type
#[allow(dead_code)]
pub type CacheEntryId = String;

/// SemanticCacheStore provides semantic similarity-based caching for LLM responses
#[derive(Clone)]
pub struct SemanticCacheStore;

impl SemanticCacheStore {
    /// Create a new SemanticCacheStore instance
    pub fn new() -> Self {
        Self
    }

    /// Initialize the semantic cache schema
    pub async fn initialize_schema(&self) -> Result<(), String> {
        let pool = chat_pool().await?;
        self.initialize_schema_with_pool(pool).await
    }

    /// Initialize schema with a provided pool (for testing)
    async fn initialize_schema_with_pool(&self, pool: &SqlitePool) -> Result<(), String> {
        // Create the main semantic_cache table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS semantic_cache (
                id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                query_text TEXT NOT NULL,
                response_text TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                hit_count INTEGER DEFAULT 0,
                similarity_threshold_used REAL
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|err| format!("failed to create semantic_cache table: {err}"))?;

        // Create index on created_at for efficient TTL cleanup
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_semantic_cache_created_at ON semantic_cache(created_at)",
        )
        .execute(pool)
        .await
        .map_err(|err| format!("failed to create semantic_cache index: {err}"))?;

        // Create the vec0 virtual table for vector similarity search
        // Using 384 dimensions which is common for embedding models
        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS semantic_cache_vec USING vec0(embedding float[384])"
        )
        .execute(pool)
        .await
        .map_err(|err| format!("failed to create semantic_cache_vec virtual table: {err}"))?;

        Ok(())
    }

    /// Look up a cached response by embedding similarity
    ///
    /// # Arguments
    /// * `embedding` - The query embedding vector (384-dimensional f32)
    /// * `threshold` - Minimum similarity threshold (0.0 to 1.0, higher = more similar)
    ///
    /// # Returns
    /// * `Ok(Some(response_text))` - If a similar query is found within threshold
    /// * `Ok(None)` - If no similar query is found
    /// * `Err(String)` - If database error occurs
    pub async fn lookup(
        &self,
        embedding: &[f32],
        threshold: f32,
    ) -> Result<Option<String>, String> {
        let pool = chat_pool().await?;
        self.lookup_with_pool(pool, embedding, threshold).await
    }

    /// Lookup with provided pool (for testing)
    async fn lookup_with_pool(
        &self,
        pool: &SqlitePool,
        embedding: &[f32],
        threshold: f32,
    ) -> Result<Option<String>, String> {
        if embedding.len() != 384 {
            return Err(format!(
                "embedding must be 384 dimensions, got {}",
                embedding.len()
            ));
        }

        // Convert embedding to JSON format for sqlite-vec
        let embedding_json = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Calculate distance threshold (sqlite-vec uses cosine distance where 0 = identical)
        // cosine_distance = 1 - cosine_similarity
        let distance_threshold = 1.0 - threshold;

        // Query for similar embeddings using vec0 virtual table
        // Use subquery to satisfy sqlite-vec KNN query requirements
        let result = sqlx::query_as::<_, (String, String, f64)>(
            r#"
            SELECT 
                sc.id,
                sc.response_text,
                vec.distance
            FROM (
                SELECT rowid, distance 
                FROM semantic_cache_vec 
                WHERE embedding MATCH ? 
                ORDER BY distance ASC 
                LIMIT 10
            ) AS vec
            JOIN semantic_cache sc ON sc.rowid = vec.rowid
            "#,
        )
        .bind(&embedding_json)
        .fetch_all(pool)
        .await
        .map_err(|err| format!("failed to lookup semantic cache: {err}"))?;

        // Filter by distance threshold and take the first match
        let result = result
            .into_iter()
            .find(|(_, _, distance)| *distance <= distance_threshold as f64);

        if let Some((id, response_text, _distance)) = result {
            // Increment hit count
            sqlx::query("UPDATE semantic_cache SET hit_count = hit_count + 1 WHERE id = ?")
                .bind(&id)
                .execute(pool)
                .await
                .map_err(|err| format!("failed to update hit count: {err}"))?;

            Ok(Some(response_text))
        } else {
            Ok(None)
        }
    }

    /// Store a new entry in the semantic cache
    ///
    /// # Arguments
    /// * `query` - The original query text
    /// * `embedding` - The query embedding vector (384-dimensional f32)
    /// * `response` - The response text to cache
    /// * `threshold` - The similarity threshold used for this entry
    ///
    /// # Returns
    /// * `Ok(())` - If stored successfully
    /// * `Err(String)` - If database error occurs
    pub async fn store(
        &self,
        query: String,
        embedding: Vec<f32>,
        response: String,
        threshold: f32,
    ) -> Result<(), String> {
        let pool = chat_pool().await?;
        self.store_with_pool(pool, query, embedding, response, threshold)
            .await
    }

    /// Store with provided pool (for testing)
    async fn store_with_pool(
        &self,
        pool: &SqlitePool,
        query: String,
        embedding: Vec<f32>,
        response: String,
        threshold: f32,
    ) -> Result<(), String> {
        if embedding.len() != 384 {
            return Err(format!(
                "embedding must be 384 dimensions, got {}",
                embedding.len()
            ));
        }

        let id = generate_cache_id();
        let created_at = unix_timestamp();

        // Convert embedding to bytes for BLOB storage
        let embedding_bytes = embedding_to_bytes(&embedding);

        // Start a transaction to ensure consistent rowid
        let mut tx = pool
            .begin()
            .await
            .map_err(|err| format!("failed to start transaction: {err}"))?;

        // Insert into main table
        sqlx::query(
            r#"
            INSERT INTO semantic_cache (id, embedding, query_text, response_text, created_at, hit_count, similarity_threshold_used)
            VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)
            "#,
        )
        .bind(&id)
        .bind(&embedding_bytes)
        .bind(&query)
        .bind(&response)
        .bind(created_at)
        .bind(threshold as f64)
        .execute(&mut *tx)
        .await
        .map_err(|err| format!("failed to store semantic cache entry: {err}"))?;

        // Insert into vec0 virtual table using last_insert_rowid() within same transaction
        let embedding_json = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        sqlx::query(
            "INSERT INTO semantic_cache_vec (rowid, embedding) VALUES (last_insert_rowid(), ?)",
        )
        .bind(&embedding_json)
        .execute(&mut *tx)
        .await
        .map_err(|err| format!("failed to store semantic cache vector: {err}"))?;

        tx.commit()
            .await
            .map_err(|err| format!("failed to commit transaction: {err}"))?;

        Ok(())
    }

    /// Invalidate (delete) a specific cache entry by ID
    ///
    /// # Arguments
    /// * `entry_id` - The ID of the entry to invalidate
    ///
    /// # Returns
    /// * `Ok(())` - If deleted successfully or entry didn't exist
    /// * `Err(String)` - If database error occurs
    pub async fn invalidate(&self, entry_id: &str) -> Result<(), String> {
        let pool = chat_pool().await?;
        self.invalidate_with_pool(pool, entry_id).await
    }

    /// Invalidate with provided pool (for testing)
    async fn invalidate_with_pool(&self, pool: &SqlitePool, entry_id: &str) -> Result<(), String> {
        // Get the rowid first
        let rowid: Option<i64> =
            sqlx::query_scalar("SELECT rowid FROM semantic_cache WHERE id = ?")
                .bind(entry_id)
                .fetch_optional(pool)
                .await
                .map_err(|err| format!("failed to find cache entry: {err}"))?;

        if let Some(rowid) = rowid {
            // Delete from vec0 virtual table first (maintains referential consistency)
            sqlx::query("DELETE FROM semantic_cache_vec WHERE rowid = ?")
                .bind(rowid)
                .execute(pool)
                .await
                .map_err(|err| format!("failed to delete vector entry: {err}"))?;

            // Delete from main table
            sqlx::query("DELETE FROM semantic_cache WHERE id = ?")
                .bind(entry_id)
                .execute(pool)
                .await
                .map_err(|err| format!("failed to delete cache entry: {err}"))?;
        }

        Ok(())
    }

    /// Clean up expired entries based on TTL (time-to-live)
    ///
    /// # Arguments
    /// * `ttl_seconds` - Time-to-live in seconds; entries older than this will be deleted
    ///
    /// # Returns
    /// * `Ok(usize)` - Number of entries deleted
    /// * `Err(String)` - If database error occurs
    pub async fn cleanup_expired(&self, ttl_seconds: i64) -> Result<usize, String> {
        let pool = chat_pool().await?;
        self.cleanup_expired_with_pool(pool, ttl_seconds).await
    }

    /// Cleanup with provided pool (for testing)
    async fn cleanup_expired_with_pool(
        &self,
        pool: &SqlitePool,
        ttl_seconds: i64,
    ) -> Result<usize, String> {
        let cutoff_time = unix_timestamp() - ttl_seconds;

        // Get rowids of expired entries
        let rowids: Vec<i64> =
            sqlx::query_scalar("SELECT rowid FROM semantic_cache WHERE created_at < ?")
                .bind(cutoff_time)
                .fetch_all(pool)
                .await
                .map_err(|err| format!("failed to find expired entries: {err}"))?;

        let deleted_count = rowids.len();

        if deleted_count > 0 {
            // Delete from vec0 virtual table
            for rowid in &rowids {
                sqlx::query("DELETE FROM semantic_cache_vec WHERE rowid = ?")
                    .bind(rowid)
                    .execute(pool)
                    .await
                    .map_err(|err| format!("failed to delete expired vector entries: {err}"))?;
            }

            // Delete from main table
            sqlx::query("DELETE FROM semantic_cache WHERE created_at < ?")
                .bind(cutoff_time)
                .execute(pool)
                .await
                .map_err(|err| format!("failed to delete expired entries: {err}"))?;
        }

        Ok(deleted_count)
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats, String> {
        let pool = chat_pool().await?;
        self.get_stats_with_pool(pool).await
    }

    /// Get stats with provided pool (for testing)
    async fn get_stats_with_pool(&self, pool: &SqlitePool) -> Result<CacheStats, String> {
        let total_entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM semantic_cache")
            .fetch_one(pool)
            .await
            .map_err(|err| format!("failed to count entries: {err}"))?;

        let total_hits: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(hit_count), 0) FROM semantic_cache")
                .fetch_one(pool)
                .await
                .map_err(|err| format!("failed to sum hits: {err}"))?;

        let oldest_entry: Option<i64> =
            sqlx::query_scalar("SELECT MIN(created_at) FROM semantic_cache")
                .fetch_optional(pool)
                .await
                .map_err(|err| format!("failed to get oldest entry: {err}"))?;

        Ok(CacheStats {
            total_entries: total_entries as usize,
            total_hits: total_hits as usize,
            oldest_entry_timestamp: oldest_entry,
        })
    }
}

impl Default for SemanticCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of entries in the cache
    pub total_entries: usize,
    /// Total number of cache hits across all entries
    pub total_hits: usize,
    /// Timestamp of the oldest entry (if any)
    pub oldest_entry_timestamp: Option<i64>,
}

/// Generate a unique cache entry ID
fn generate_cache_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = unix_timestamp();

    format!("cache_{}_{}", timestamp, counter)
}

/// Convert f32 embedding vector to bytes for BLOB storage
fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Convert bytes back to f32 embedding vector
#[allow(dead_code)]
#[allow(clippy::chunks_exact_to_as_chunks)]
fn bytes_to_embedding(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("invalid embedding bytes length".to_string());
    }

    let mut embedding = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let bytes_array: [u8; 4] = chunk.try_into().map_err(|_| "chunk conversion failed")?;
        embedding.push(f32::from_le_bytes(bytes_array));
    }

    Ok(embedding)
}

/// Get current Unix timestamp
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
        // Initialize the vec extension
        crate::chat::ensure_vec_extension_initialized();

        SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("failed to create test pool")
    }

    fn create_test_embedding(seed: f32) -> Vec<f32> {
        // Create a 384-dimensional embedding with the seed value
        vec![seed; 384]
    }

    #[tokio::test]
    async fn test_schema_initialization() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Verify tables exist by running a simple query
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='semantic_cache'",
        )
        .fetch_one(&pool)
        .await
        .expect("failed to check table existence");

        assert_eq!(count, 1, "semantic_cache table should exist");
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Store an entry
        let embedding = create_test_embedding(0.5);
        let query = "test query".to_string();
        let response = "test response".to_string();

        store
            .store_with_pool(
                &pool,
                query.clone(),
                embedding.clone(),
                response.clone(),
                0.8,
            )
            .await
            .expect("failed to store");

        // Lookup with same embedding (should find it)
        let found = store
            .lookup_with_pool(&pool, &embedding, 0.8)
            .await
            .expect("failed to lookup");

        assert_eq!(found, Some(response));
    }

    #[tokio::test]
    async fn test_lookup_threshold() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Store an entry
        let embedding = create_test_embedding(0.5);
        let query = "test query".to_string();
        let response = "test response".to_string();

        store
            .store_with_pool(&pool, query, embedding.clone(), response, 0.8)
            .await
            .expect("failed to store");

        // Lookup with very high threshold (should not find it due to exact match requirement)
        let different_embedding = create_test_embedding(0.9);
        let found = store
            .lookup_with_pool(&pool, &different_embedding, 0.99)
            .await
            .expect("failed to lookup");

        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn test_invalidate() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Store an entry
        let embedding = create_test_embedding(0.5);
        let query = "test query".to_string();
        let response = "test response".to_string();

        store
            .store_with_pool(&pool, query, embedding.clone(), response.clone(), 0.8)
            .await
            .expect("failed to store");

        // Verify it exists
        let found = store
            .lookup_with_pool(&pool, &embedding, 0.8)
            .await
            .expect("failed to lookup");
        assert!(found.is_some());

        // Get the entry ID and invalidate it
        let id: String = sqlx::query_scalar("SELECT id FROM semantic_cache LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("failed to get id");

        store
            .invalidate_with_pool(&pool, &id)
            .await
            .expect("failed to invalidate");

        // Verify it's gone
        let found = store
            .lookup_with_pool(&pool, &embedding, 0.8)
            .await
            .expect("failed to lookup");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Insert an entry with an old timestamp directly
        sqlx::query(
            "INSERT INTO semantic_cache (id, embedding, query_text, response_text, created_at, hit_count, similarity_threshold_used)
             VALUES ('old_entry', X'0000', 'old query', 'old response', 1, 0, 0.8)"
        )
        .execute(&pool)
        .await
        .expect("failed to insert old entry");

        // Verify entry exists
        let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM semantic_cache")
            .fetch_one(&pool)
            .await
            .expect("failed to count");
        assert_eq!(count_before, 1);

        // Clean up with TTL of 3600 (should delete entries older than 1 hour)
        let deleted = store
            .cleanup_expired_with_pool(&pool, 3600)
            .await
            .expect("failed to cleanup");

        assert_eq!(deleted, 1);

        // Verify stats show 0 entries
        let stats = store
            .get_stats_with_pool(&pool)
            .await
            .expect("failed to get stats");
        assert_eq!(stats.total_entries, 0);
    }

    #[tokio::test]
    async fn test_hit_count_increment() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Store an entry
        let embedding = create_test_embedding(0.5);
        let query = "test query".to_string();
        let response = "test response".to_string();

        store
            .store_with_pool(&pool, query, embedding.clone(), response, 0.8)
            .await
            .expect("failed to store");

        // Lookup multiple times
        for _ in 0..3 {
            let _ = store
                .lookup_with_pool(&pool, &embedding, 0.8)
                .await
                .expect("failed to lookup");
        }

        // Verify hit count
        let stats = store
            .get_stats_with_pool(&pool)
            .await
            .expect("failed to get stats");
        assert_eq!(stats.total_hits, 3);
    }

    #[tokio::test]
    async fn test_embedding_dimension_validation() {
        let pool = create_test_pool().await;
        let store = SemanticCacheStore::new();

        store
            .initialize_schema_with_pool(&pool)
            .await
            .expect("failed to initialize schema");

        // Try to store with wrong dimensions
        let wrong_embedding = vec![0.5; 100]; // Only 100 dimensions
        let result = store
            .store_with_pool(
                &pool,
                "query".to_string(),
                wrong_embedding,
                "response".to_string(),
                0.8,
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("384"));

        // Try to lookup with wrong dimensions
        let wrong_embedding = vec![0.5; 100];
        let result = store.lookup_with_pool(&pool, &wrong_embedding, 0.8).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("384"));
    }
}
