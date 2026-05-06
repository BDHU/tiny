use anyhow::Result;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tiny::Tool;
use tokio::process::Command;

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
