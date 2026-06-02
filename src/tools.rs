use tiny::ErasedTool;

use crate::toolset;

pub fn default_tools() -> Vec<Box<dyn ErasedTool>> {
    toolset::workspace_tools()
}
