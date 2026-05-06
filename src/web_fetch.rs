use anyhow::{Context, Result};

use crate::web::client;

pub async fn fetch(url: &str) -> Result<String> {
    let endpoint = format!("https://r.jina.ai/{url}");
    let mut req = client().get(&endpoint).header("Accept", "text/plain");
    if let Some(key) = std::env::var("JINA_API_KEY").ok().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }

    req.send()
        .await
        .context("send fetch request")?
        .error_for_status()
        .context("fetch returned error status")?
        .text()
        .await
        .context("read response body")
}
