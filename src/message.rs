use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool(ToolResult),
}
