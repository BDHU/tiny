use crate::message::Message;
use crate::tool::ErasedTool;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Box<dyn ErasedTool>],
    ) -> Result<Message>;
}
