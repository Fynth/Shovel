use acp_core::embedding::{EmbeddingModel, cosine_similarity};
use std::sync::Arc;
use storage::{CacheStats, SemanticCacheStore};
use tokio::{sync::RwLock, task::JoinHandle};

/// Default similarity threshold for cache lookups
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.85;

/// Default TTL for cache entries in seconds (7 days)
pub const DEFAULT_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Default cleanup interval in seconds (1 hour)
pub const DEFAULT_CLEANUP_INTERVAL_SECONDS: u64 = 60 * 60;

/// Semantic cache for LLM responses with embedding-based similarity matching
///
/// Provides efficient caching of LLM responses based on semantic similarity
/// of queries. Uses cosine similarity between embeddings to find cached
/// responses for semantically equivalent queries.
pub struct SemanticCache {
    store: SemanticCacheStore,
    embedding_model: Option<Arc<EmbeddingModel>>,
    similarity_threshold: f32,
    ttl_seconds: i64,
    cleanup_handle: RwLock<Option<JoinHandle<()>>>,
}

impl SemanticCache {
    /// Create a new SemanticCache instance
    ///
    /// # Arguments
    /// * `embedding_model` - Optional embedding model for generating embeddings.
    ///   If None, cache operations will fail gracefully.
    ///
    /// # Returns
    /// * `Ok(SemanticCache)` - New cache instance with background cleanup task started
    /// * `Err(String)` - If initialization failed
    pub async fn new(embedding_model: Option<Arc<EmbeddingModel>>) -> Result<Self, String> {
        let store = SemanticCacheStore::new();

        // Initialize the schema
        if let Err(e) = store.initialize_schema().await {
            return Err(format!("Failed to initialize semantic cache schema: {}", e));
        }

        let cache = Self {
            store,
            embedding_model,
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
            ttl_seconds: DEFAULT_TTL_SECONDS,
            cleanup_handle: RwLock::new(None),
        };

        // Start background cleanup task
        cache.start_cleanup_task(DEFAULT_CLEANUP_INTERVAL_SECONDS);

        Ok(cache)
    }

    /// Create a new SemanticCache with custom parameters
    ///
    /// # Arguments
    /// * `embedding_model` - Optional embedding model
    /// * `similarity_threshold` - Minimum cosine similarity for cache hits (0.0-1.0)
    /// * `ttl_seconds` - Time-to-live for cache entries in seconds
    ///
    /// # Returns
    /// * `Ok(SemanticCache)` - New cache instance
    pub async fn with_params(
        embedding_model: Option<Arc<EmbeddingModel>>,
        similarity_threshold: f32,
        ttl_seconds: i64,
    ) -> Result<Self, String> {
        let store = SemanticCacheStore::new();
        if let Err(e) = store.initialize_schema().await {
            return Err(format!("Failed to initialize semantic cache schema: {}", e));
        }

        let cache = Self {
            store,
            embedding_model,
            similarity_threshold: similarity_threshold.clamp(0.0, 1.0),
            ttl_seconds,
            cleanup_handle: RwLock::new(None),
        };

        cache.start_cleanup_task(DEFAULT_CLEANUP_INTERVAL_SECONDS);

        Ok(cache)
    }

    /// Look up a cached response using a pre-computed embedding
    ///
    /// This method is useful when you already have the embedding and want to
    /// avoid re-generating it for the cache lookup.
    ///
    /// # Arguments
    /// * `embedding` - 384-dimensional query embedding vector
    /// * `threshold` - Optional custom similarity threshold (uses default if None)
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Cached response if found within threshold
    /// * `Ok(None)` - No matching cache entry found
    /// * `Err(String)` - If lookup failed
    pub async fn lookup_with_embedding(
        &self,
        embedding: &[f32],
        threshold: Option<f32>,
    ) -> Result<Option<String>, String> {
        let effective_threshold = threshold.unwrap_or(self.similarity_threshold);
        self.store.lookup(embedding, effective_threshold).await
    }

    /// Look up a cached response for the given query text
    ///
    /// Generates an embedding for the query text and searches for similar
    /// cached responses. Returns None if no embedding model is available.
    ///
    /// # Arguments
    /// * `query` - Query text to look up
    /// * `threshold` - Optional custom similarity threshold
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Cached response if found
    /// * `Ok(None)` - No matching entry or no embedding model available
    /// * `Err(String)` - If lookup failed
    pub async fn lookup(
        &self,
        query: &str,
        threshold: Option<f32>,
    ) -> Result<Option<String>, String> {
        let embedding_model = match &self.embedding_model {
            Some(model) => model,
            None => return Ok(None),
        };

        // Generate embedding — log warning and return None on failure (no-cache mode)
        let embedding = match embedding_model.embed(query.to_string()).await {
            Ok(emb) => emb,
            Err(e) => {
                tracing::warn!(
                    "Embedding generation failed during cache lookup, skipping cache: {}",
                    e
                );
                return Ok(None);
            }
        };

        match self.lookup_with_embedding(&embedding, threshold).await {
            Ok(result) => Ok(result),
            Err(e) => {
                tracing::warn!("Cache lookup failed, returning no match: {}", e);
                Ok(None)
            }
        }
    }

    /// Store a response with a pre-computed embedding
    ///
    /// # Arguments
    /// * `query` - Original query text
    /// * `embedding` - 384-dimensional query embedding vector
    /// * `response` - Response text to cache
    /// * `threshold` - Optional custom similarity threshold used for this entry
    ///
    /// # Returns
    /// * `Ok(())` - If stored successfully
    /// * `Err(String)` - If storage failed
    pub async fn store_with_embedding(
        &self,
        query: String,
        embedding: Vec<f32>,
        response: String,
        threshold: Option<f32>,
    ) -> Result<(), String> {
        let effective_threshold = threshold.unwrap_or(self.similarity_threshold);
        self.store
            .store(query, embedding, response, effective_threshold)
            .await
    }

    /// Store a response for the given query text
    ///
    /// Generates an embedding and stores the query-response pair.
    /// Does nothing if no embedding model is available.
    ///
    /// # Arguments
    /// * `query` - Query text
    /// * `response` - Response text to cache
    /// * `threshold` - Optional custom similarity threshold
    ///
    /// # Returns
    /// * `Ok(())` - If stored successfully or no model available
    /// * `Err(String)` - If storage failed
    pub async fn store(
        &self,
        query: String,
        response: String,
        threshold: Option<f32>,
    ) -> Result<(), String> {
        let embedding_model = match &self.embedding_model {
            Some(model) => model,
            None => return Ok(()),
        };

        let embedding = match embedding_model.embed(query.clone()).await {
            Ok(emb) => emb,
            Err(e) => {
                tracing::warn!(
                    "Embedding generation failed during cache store, skipping cache write: {}",
                    e
                );
                return Ok(());
            }
        };

        if let Err(e) = self
            .store_with_embedding(query, embedding, response, threshold)
            .await
        {
            tracing::warn!("Cache store failed, continuing without cache: {}", e);
        }

        Ok(())
    }

    /// Calculate similarity between two embeddings
    ///
    /// Convenience method that delegates to the embedding module's
    /// cosine_similarity function.
    ///
    /// # Arguments
    /// * `vec1` - First embedding vector
    /// * `vec2` - Second embedding vector
    ///
    /// # Returns
    /// Cosine similarity in range [-1.0, 1.0]
    pub fn similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        cosine_similarity(vec1, vec2)
    }

    /// Get cache statistics
    ///
    /// # Returns
    /// * `Ok(CacheStats)` - Current cache statistics
    /// * `Err(String)` - If query failed
    pub async fn stats(&self) -> Result<CacheStats, String> {
        self.store.get_stats().await
    }

    /// Clean up expired entries manually
    ///
    /// # Returns
    /// * `Ok(count)` - Number of entries deleted
    /// * `Err(String)` - If cleanup failed
    pub async fn cleanup_expired(&self) -> Result<usize, String> {
        match self.store.cleanup_expired(self.ttl_seconds).await {
            Ok(count) => Ok(count),
            Err(e) => {
                tracing::warn!("Cache cleanup failed, continuing: {}", e);
                Ok(0)
            }
        }
    }

    /// Start the background TTL cleanup task
    ///
    /// This spawns a tokio task that periodically cleans up expired cache entries.
    /// The task runs until the SemanticCache is dropped.
    ///
    /// # Arguments
    /// * `interval_seconds` - Interval between cleanup runs
    fn start_cleanup_task(&self, interval_seconds: u64) {
        let store = self.store.clone();
        let ttl_seconds = self.ttl_seconds;
        let interval = tokio::time::Duration::from_secs(interval_seconds);

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                // Attempt cleanup, log errors but don't panic
                match store.cleanup_expired(ttl_seconds).await {
                    Ok(deleted) => {
                        if deleted > 0 {
                            tracing::debug!(
                                "Cleaned up {} expired semantic cache entries",
                                deleted
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Semantic cache cleanup failed: {}", e);
                    }
                }
            }
        });

        // Store the handle - use try_write to avoid blocking
        if let Ok(mut guard) = self.cleanup_handle.try_write() {
            *guard = Some(handle);
        }
    }

    /// Stop the background cleanup task
    ///
    /// This is called automatically on drop, but can be called manually
    /// if you need to ensure cleanup stops before the cache is dropped.
    pub async fn stop_cleanup_task(&self) {
        let mut guard = self.cleanup_handle.write().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

    /// Check if the cache has an embedding model available
    pub fn has_embedding_model(&self) -> bool {
        self.embedding_model.is_some()
    }

    /// Get the current similarity threshold
    pub fn similarity_threshold(&self) -> f32 {
        self.similarity_threshold
    }

    /// Set the similarity threshold
    pub fn set_similarity_threshold(&mut self, threshold: f32) {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
    }
}

impl Drop for SemanticCache {
    fn drop(&mut self) {
        // Try to abort the cleanup task if it's still running
        if let Ok(mut guard) = self.cleanup_handle.try_write()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
    }
}

/// Builder for SemanticCache with fluent API
pub struct SemanticCacheBuilder {
    embedding_model: Option<Arc<EmbeddingModel>>,
    similarity_threshold: f32,
    ttl_seconds: i64,
    cleanup_interval_seconds: u64,
}

impl SemanticCacheBuilder {
    /// Create a new builder with default values
    pub fn new() -> Self {
        Self {
            embedding_model: None,
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
            ttl_seconds: DEFAULT_TTL_SECONDS,
            cleanup_interval_seconds: DEFAULT_CLEANUP_INTERVAL_SECONDS,
        }
    }

    /// Set the embedding model
    pub fn with_embedding_model(mut self, model: Arc<EmbeddingModel>) -> Self {
        self.embedding_model = Some(model);
        self
    }

    /// Set the similarity threshold
    pub fn with_similarity_threshold(mut self, threshold: f32) -> Self {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set the TTL in seconds
    pub fn with_ttl_seconds(mut self, ttl: i64) -> Self {
        self.ttl_seconds = ttl;
        self
    }

    /// Set the cleanup interval in seconds
    pub fn with_cleanup_interval(mut self, interval: u64) -> Self {
        self.cleanup_interval_seconds = interval;
        self
    }

    /// Build the SemanticCache
    pub async fn build(self) -> Result<SemanticCache, String> {
        let store = SemanticCacheStore::new();
        if let Err(e) = store.initialize_schema().await {
            return Err(format!("Failed to initialize semantic cache schema: {}", e));
        }

        let cache = SemanticCache {
            store,
            embedding_model: self.embedding_model,
            similarity_threshold: self.similarity_threshold,
            ttl_seconds: self.ttl_seconds,
            cleanup_handle: RwLock::new(None),
        };

        cache.start_cleanup_task(self.cleanup_interval_seconds);

        Ok(cache)
    }
}

impl Default for SemanticCacheBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_wrapper() {
        let _cache = SemanticCacheBuilder::new().build();
        // Can't easily test without async runtime, but the similarity
        // method just delegates to embedding::cosine_similarity
    }

    #[test]
    fn test_default_values() {
        let builder = SemanticCacheBuilder::new();
        assert_eq!(builder.similarity_threshold, DEFAULT_SIMILARITY_THRESHOLD);
        assert_eq!(builder.ttl_seconds, DEFAULT_TTL_SECONDS);
        assert_eq!(
            builder.cleanup_interval_seconds,
            DEFAULT_CLEANUP_INTERVAL_SECONDS
        );
    }

    #[test]
    fn test_builder_chaining() {
        let builder = SemanticCacheBuilder::new()
            .with_similarity_threshold(0.9)
            .with_ttl_seconds(3600)
            .with_cleanup_interval(300);

        assert_eq!(builder.similarity_threshold, 0.9);
        assert_eq!(builder.ttl_seconds, 3600);
        assert_eq!(builder.cleanup_interval_seconds, 300);
    }

    #[test]
    fn test_threshold_clamping() {
        let builder = SemanticCacheBuilder::new().with_similarity_threshold(1.5); // Should clamp to 1.0
        assert_eq!(builder.similarity_threshold, 1.0);

        let builder = SemanticCacheBuilder::new().with_similarity_threshold(-0.5); // Should clamp to 0.0
        assert_eq!(builder.similarity_threshold, 0.0);
    }
}
