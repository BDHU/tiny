use std::time::Duration;

use anyhow::{Context, Result};

use crate::html::decode_entities;

const USER_AGENT: &str = "Mozilla/5.0 (compatible; tiny-agent/0.1)";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

pub async fn fetch(url: &str) -> Result<String> {
    let response = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build http client")?
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .context("send fetch request")?
        .error_for_status()
        .context("fetch returned error status")?;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut body = response.text().await.context("read response body")?;
    if body.len() > MAX_RESPONSE_BYTES {
        let cut = body
            .char_indices()
            .take_while(|(i, _)| *i < MAX_RESPONSE_BYTES)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        body.truncate(cut);
    }

    if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        Ok(tidy(&to_text(&body)))
    } else {
        Ok(body)
    }
}

fn to_text(html: &str) -> String {
    let stripped = remove_blocks(html, &["script", "style", "noscript"]);
    let mut out = String::with_capacity(stripped.len());
    let mut in_tag = false;
    for ch in stripped.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn remove_blocks(html: &str, tags: &[&str]) -> String {
    let mut out = html.to_string();
    for tag in tags {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        loop {
            let lower = out.to_ascii_lowercase();
            let Some(start) = lower.find(&open) else { break };
            let end = match lower[start..].find(&close) {
                Some(rel) => start + rel + close.len(),
                None => out.len(),
            };
            out.replace_range(start..end, "");
        }
    }
    out
}

fn tidy(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
