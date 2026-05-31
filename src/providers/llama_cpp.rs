use super::openai_compatible::OpenAiCompatibleProvider;
use crate::agent::{Message, Provider};
use crate::tool::ErasedTool;
use anyhow::Result;
use async_trait::async_trait;

pub struct LlamaCppProvider {
    inner: OpenAiCompatibleProvider,
}

impl LlamaCppProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new("llama.cpp", base_url, None, model),
        }
    }
}

#[async_trait]
impl Provider for LlamaCppProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message> {
        self.inner.complete(system, messages, tools).await
    }
}
