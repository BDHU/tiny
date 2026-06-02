use tiny::ErasedTool;

use crate::toolset;

use super::registry::{SubagentSpec, ToolsetKind};

pub(crate) fn for_subagent(spec: &SubagentSpec) -> Vec<Box<dyn ErasedTool>> {
    match spec.toolset {
        ToolsetKind::ReadOnly => toolset::read_only_tools(),
    }
}
