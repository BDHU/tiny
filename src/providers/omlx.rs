use super::openai_compatible::OpenAiCompatibleProvider;
use crate::agent::{Message, Provider};
use crate::tool::ErasedTool;
use anyhow::Result;
use async_trait::async_trait;

pub struct OmlxProvider {
    inner: OpenAiCompatibleProvider,
}

impl OmlxProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            inner: OpenAiCompatibleProvider::new("omlx", base_url, api_key, model),
        }
    }
}

#[async_trait]
impl Provider for OmlxProvider {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message> {
        self.inner.complete(system, messages, tools).await
    }
}
