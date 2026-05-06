use std::fmt::Write;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tiny::{boxed_tool, ErasedTool, Tool};
use tokio::fs;
use tokio::process::Command;

use crate::{web_fetch, web_search};

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

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Path to the file.
    path: String,
}

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    type Args = ReadArgs;

    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read the contents of a file from disk."
    }
    async fn call(&self, args: ReadArgs) -> Result<String> {
        Ok(fs::read_to_string(args.path).await?)
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Path to the file.
    path: String,
    /// Content to write.
    content: String,
}

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    type Args = WriteArgs;

    fn name(&self) -> &str {
        "write"
    }
    fn description(&self) -> &str {
        "Write content to a file, overwriting any existing file at the path."
    }
    async fn call(&self, args: WriteArgs) -> Result<String> {
        let bytes = args.content.len();
        fs::write(&args.path, args.content).await?;
        Ok(format!("wrote {bytes} bytes to {}", args.path))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Path to the file.
    path: String,
    /// Exact text to replace. It must appear exactly once.
    old_string: String,
    /// Replacement text.
    new_string: String,
}

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    type Args = EditArgs;

    fn name(&self) -> &str {
        "edit"
    }
    fn description(&self) -> &str {
        "Replace an exact string in a file. The old string must appear exactly once."
    }
    async fn call(&self, args: EditArgs) -> Result<String> {
        let original = fs::read_to_string(&args.path).await?;
        let count = original.matches(&args.old_string).count();
        if count == 0 {
            return Err(anyhow!("old_string not found in {}", args.path));
        }
        if count > 1 {
            return Err(anyhow!(
                "old_string appears {count} times in {}; provide more context to make it unique",
                args.path
            ));
        }
        let updated = original.replacen(&args.old_string, &args.new_string, 1);
        fs::write(&args.path, updated).await?;
        Ok(format!("replaced 1 occurrence in {}", args.path))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Directory path. Defaults to the current directory.
    path: Option<String>,
}

pub struct ListTool;

#[async_trait]
impl Tool for ListTool {
    type Args = ListArgs;

    fn name(&self) -> &str {
        "list"
    }
    fn description(&self) -> &str {
        "List the entries of a directory."
    }
    async fn call(&self, args: ListArgs) -> Result<String> {
        let path = args.path.as_deref().unwrap_or(".");
        let mut read_dir = fs::read_dir(path).await?;
        let mut entries = Vec::new();

        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let suffix = if entry.file_type().await?.is_dir() {
                "/"
            } else {
                ""
            };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort();
        Ok(entries.join("\n"))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct BashArgs {
    /// Shell command to execute.
    command: String,
}

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    type Args = BashArgs;

    fn name(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Run a shell command via /bin/sh -c and return combined stdout/stderr and exit status."
    }
    async fn call(&self, args: BashArgs) -> Result<String> {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(args.command)
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = output.status.code().unwrap_or(-1);
        Ok(format!(
            "exit: {status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// Query string.
    query: String,
    /// Maximum number of results to return. Defaults to 5.
    limit: Option<usize>,
}

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    type Args = WebSearchArgs;

    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the public web via DuckDuckGo and return ranked title/url/snippet results."
    }
    async fn call(&self, args: WebSearchArgs) -> Result<String> {
        let limit = args.limit.unwrap_or(5).clamp(1, 20);
        let results = web_search::search(&args.query, limit).await?;
        if results.is_empty() {
            return Ok("(no results)".to_string());
        }
        let mut out = String::new();
        for (i, r) in results.iter().enumerate() {
            let _ = writeln!(out, "{}. {}\n   {}\n   {}", i + 1, r.title, r.url, r.snippet);
        }
        Ok(out.trim_end().to_string())
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct WebFetchArgs {
    /// URL to fetch (must include scheme, e.g. https://...).
    url: String,
    /// Maximum number of characters to return. Defaults to 8000.
    max_chars: Option<usize>,
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    type Args = WebFetchArgs;

    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch a URL and return its readable text. HTML pages are stripped of markup; other content is returned as-is."
    }
    async fn call(&self, args: WebFetchArgs) -> Result<String> {
        let max_chars = args.max_chars.unwrap_or(8000).clamp(100, 100_000);
        let body = web_fetch::fetch(&args.url).await?;
        Ok(truncate_chars(body, max_chars))
    }
}

fn truncate_chars(mut s: String, max_chars: usize) -> String {
    let mut indices = s.char_indices();
    if let Some((cut, _)) = indices.nth(max_chars) {
        s.truncate(cut);
        s.push_str("\n…[truncated]");
    }
    s
}
