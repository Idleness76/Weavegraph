use crate::message::Message;
use async_trait::async_trait;
use futures_util::stream::BoxStream;

/// Unified error type for framework-agnostic LLM providers.
pub type LlmError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Completed response from an LLM provider.
#[derive(Clone, Debug, Default)]
pub struct LlmResponse {
    pub content: String,
    pub metadata: serde_json::Value,
}

/// Trait for non-streaming LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Execute a chat completion request over the provided message history.
    async fn chat(&self, messages: &[Message]) -> Result<LlmResponse, LlmError>;
}

/// Trait for streaming LLM providers.
#[async_trait]
pub trait LlmStreamProvider: LlmProvider {
    /// Stream chunk type produced by the provider.
    type Chunk: Send + 'static;

    /// Execute a streaming chat completion request.
    async fn chat_stream(
        &self,
        messages: &[Message],
    ) -> Result<BoxStream<'static, Result<Self::Chunk, LlmError>>, LlmError>;
}
