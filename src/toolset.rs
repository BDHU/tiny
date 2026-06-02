pub(crate) mod fs;
pub(crate) mod shell;
pub(crate) mod web;

use tiny::{boxed_tool, ErasedTool};

use fs::{EditTool, GlobTool, GrepTool, ListTool, ReadTool, WriteTool};
use shell::BashTool;
use web::{WebFetchTool, WebSearchTool};

pub(crate) fn workspace_tools() -> Vec<Box<dyn ErasedTool>> {
    let mut tools = read_only_tools();
    tools.extend([
        boxed_tool(WriteTool),
        boxed_tool(EditTool),
        boxed_tool(BashTool),
        boxed_tool(WebSearchTool),
        boxed_tool(WebFetchTool),
    ]);
    tools
}

pub(crate) fn read_only_tools() -> Vec<Box<dyn ErasedTool>> {
    vec![
        boxed_tool(ReadTool),
        boxed_tool(ListTool),
        boxed_tool(GlobTool),
        boxed_tool(GrepTool),
    ]
}
