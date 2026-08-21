pub mod agents;
pub mod deepseek;
#[cfg(feature = "embedding")]
pub mod embedding;
pub mod ollama;
pub mod runtime;

pub use acp_registry::{install_acp_registry_agent, load_acp_registry_agents};
pub use agents::{
    AgentCoordinator, DataAnalyst, HandoffRecord, IntentClassifier, SchemaArchitect, Specialist,
    SpecialistResponse, SqlExpert, UserIntent,
};
pub use deepseek::{
    EmbeddedDeepSeekAgentConfig, build_embedded_deepseek_launch, run_embedded_deepseek_agent,
};
#[cfg(feature = "embedding")]
pub use embedding::{
    EMBEDDING_DIM, EmbeddingModel, LazyEmbeddingModel, MODEL_FILENAME, cosine_similarity,
};
pub use ollama::{
    EmbeddedOllamaAgentConfig, OllamaSpecialistAdapter, build_embedded_ollama_launch,
    load_ollama_models, run_embedded_ollama_agent,
};
pub use runtime::{
    cancel_acp_prompt, clear_active_specialist, connect_acp_agent, disconnect_acp_agent,
    drain_acp_events, get_active_specialist, record_execution, respond_acp_permission,
    route_acp_request, send_acp_prompt, send_acp_prompt_with_routing,
};
