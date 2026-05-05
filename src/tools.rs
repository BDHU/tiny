use anyhow::{anyhow, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tiny::{boxed_tool, ErasedTool, Tool};
use tokio::fs;
use tokio::process::Command;

pub fn default_tools() -> Vec<Box<dyn ErasedTool>> {
    vec![
        boxed_tool(ReadTool),
        boxed_tool(WriteTool),
        boxed_tool(EditTool),
        boxed_tool(ListTool),
        boxed_tool(BashTool),
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
