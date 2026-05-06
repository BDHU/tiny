use tiny::{boxed_tool, ErasedTool};

use crate::toolset::{
    fs::{EditTool, ListTool, ReadTool, WriteTool},
    shell::BashTool,
    web::{WebFetchTool, WebSearchTool},
};

pub fn default_tools() -> Vec<Box<dyn ErasedTool>> {
    vec![
        boxed_tool(ReadTool),
        boxed_tool(WriteTool),
        boxed_tool(EditTool),
        boxed_tool(ListTool),
        boxed_tool(BashTool),
        boxed_tool(WebSearchTool),
        boxed_tool(WebFetchTool),
    ]
}
