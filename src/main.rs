use anyhow::{Context, Result};
use tiny::{AnthropicProvider, Message, Provider};

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("set ANTHROPIC_API_KEY in your environment")?;
    let provider = AnthropicProvider::new(api_key, "claude-sonnet-4-5");

    let messages = vec![Message::user_text("Say hi in five words or fewer.")];
    let reply = provider.complete("You are a terse assistant.", &messages, &[]).await?;
    println!("{}", reply.text_concat());
    Ok(())
}
