//! AI provider trait + Mock + OpenAI-compatible implementations.

mod mock;
mod openai;
#[allow(clippy::module_inception)]
#[path = "trait.rs"]
mod provider_trait;

pub use mock::MockAiProvider;
pub use openai::{OpenAiCompatibleProvider, DEFAULT_TIMEOUT_SECS};
pub use provider_trait::{
    host_from_base_url, is_loopback_host, is_remote_base_url, AiProvider, AiProviderKind,
    ChatMessage, ChatRole, CompletionRequest, CompletionResponse, TokenUsage,
};
