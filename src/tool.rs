use anyhow::{Context, Result};
use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[async_trait]
pub trait Tool: Send + Sync {
    type Args: DeserializeOwned + JsonSchema + Send;

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn call(&self, args: Self::Args) -> Result<String>;
}

#[async_trait]
pub trait ErasedTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn call(&self, input: Value) -> Result<String>;
}

pub fn boxed_tool(tool: impl Tool + 'static) -> Box<dyn ErasedTool> {
    Box::new(ToolAdapter(tool))
}

struct ToolAdapter<T>(T);

#[async_trait]
impl<T> ErasedTool for ToolAdapter<T>
where
    T: Tool + 'static,
{
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn input_schema(&self) -> Value {
        let mut schema = serde_json::to_value(schema_for!(T::Args)).expect("schema is JSON");
        if let Value::Object(object) = &mut schema {
            object.remove("$schema");
        }
        schema
    }

    async fn call(&self, input: Value) -> Result<String> {
        let args = serde_json::from_value(input)
            .with_context(|| format!("parse arguments for tool '{}'", self.name()))?;
        self.0.call(args).await
    }
}
