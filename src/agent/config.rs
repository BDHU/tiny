use std::sync::Arc;

use crate::compact;
use crate::tool::{boxed_tool, ErasedTool, Tool};

use super::Provider;

pub struct AgentConfig {
    pub(crate) provider: Arc<dyn Provider>,
    pub(crate) tools: Vec<Box<dyn ErasedTool>>,
    pub(crate) system: String,
    pub(crate) compact_threshold: usize,
    pub(crate) auto_allow_tools: bool,
}

impl AgentConfig {
    pub fn new(provider: impl Provider + 'static, system: impl Into<String>) -> Self {
        Self::new_with_provider(Arc::new(provider), system)
    }

    pub fn new_with_provider(provider: Arc<dyn Provider>, system: impl Into<String>) -> Self {
        Self {
            provider,
            tools: Vec::new(),
            system: system.into(),
            compact_threshold: compact::DEFAULT_THRESHOLD,
            auto_allow_tools: false,
        }
    }

    pub fn with_tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(boxed_tool(tool));
        self
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Box<dyn ErasedTool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn with_compact_threshold(mut self, chars: usize) -> Self {
        self.compact_threshold = chars;
        self
    }

    pub fn with_auto_allow_tools(mut self) -> Self {
        self.auto_allow_tools = true;
        self
    }
}
