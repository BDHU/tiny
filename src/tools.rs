use std::sync::Arc;

use tiny::{boxed_tool, ErasedTool, Provider};

use crate::{subagent::DelegateTool, toolset};

pub fn default_tools(provider: Arc<dyn Provider>) -> Vec<Box<dyn ErasedTool>> {
    let mut tools = toolset::workspace_tools();
    tools.push(boxed_tool(DelegateTool::new(provider)));
    tools
}
