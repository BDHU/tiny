use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tiny::{Provider, Tool};

use super::{registry, runner};

#[derive(Deserialize, JsonSchema)]
pub(crate) struct DelegateArgs {
    /// Built-in subagent name, e.g. "reviewer".
    agent: String,
    /// Focused task for the subagent.
    task: String,
}

pub(crate) struct DelegateTool {
    provider: Arc<dyn Provider>,
}

impl DelegateTool {
    pub(crate) fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    type Args = DelegateArgs;

    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        registry::delegate_description()
    }

    async fn call(&self, args: DelegateArgs) -> Result<String> {
        let spec = registry::find(&args.agent)?;
        runner::run(self.provider.clone(), spec, args.task).await
    }
}
