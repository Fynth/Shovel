// Core ACP functionality (no DB dependencies) — re-exported from acp-core.
pub use acp_core::{
    agents::{
        self, AgentCoordinator, DataAnalyst, HandoffRecord, IntentClassifier, SchemaArchitect,
        Specialist, SpecialistResponse, SqlExpert, UserIntent,
    },
    deepseek::{
        self, EmbeddedDeepSeekAgentConfig, build_embedded_deepseek_launch,
        run_embedded_deepseek_agent,
    },
    ollama::{
        self, EmbeddedOllamaAgentConfig, OllamaSpecialistAdapter, build_embedded_ollama_launch,
        load_ollama_models, run_embedded_ollama_agent,
    },
    runtime::{
        self, cancel_acp_prompt, clear_active_specialist, connect_acp_agent, disconnect_acp_agent,
        drain_acp_events, get_active_specialist, record_execution, respond_acp_permission,
        route_acp_request, send_acp_prompt, send_acp_prompt_with_routing,
    },
};

#[cfg(feature = "embedding")]
pub use acp_core::embedding::{
    EMBEDDING_DIM, EmbeddingModel, LazyEmbeddingModel, MODEL_FILENAME, cosine_similarity,
};

pub use acp_registry::{install_acp_registry_agent, load_acp_registry_agents};

// DB-dependent modules that live only in `acp`.
pub mod context;
pub mod introspection;
#[cfg(feature = "embedding")]
pub mod semantic_cache;

pub use context::{build_acp_database_context, warm_acp_database_schema_context};
pub use introspection::{
    ActiveQueryInfo, ColumnInfo, IndexInfo, IndexStat, IntrospectionConfig, IntrospectionPool,
    IntrospectionRateLimiter, IntrospectionResult, LockInfo, QueryHistoryEntry, SchemaInfo,
    TableInfo, TableStat, explain_query_plan_mysql, explain_query_plan_postgres,
    explain_query_plan_sqlite,
};
#[cfg(feature = "embedding")]
pub use semantic_cache::{SemanticCache, SemanticCacheBuilder};
