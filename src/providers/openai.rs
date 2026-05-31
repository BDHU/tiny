use super::openai_compatible::OpenAiCompatibleProvider;
use crate::agent::{Message, Provider};
use crate::tool::ErasedTool;
use anyhow::Result;
use async_trait::async_trait;

pub struct OpenAiProvider {
    inner: OpenAiCompatibleProvider,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new(
                "openai",
                "https://api.openai.com",
                Some(api_key.into()),
                model,
            ),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message> {
        self.inner.complete(system, messages, tools).await
    }
}
