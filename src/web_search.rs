use std::time::Duration;

use anyhow::{Context, Result};

use crate::html::decode_entities;

const ENDPOINT: &str = "https://lite.duckduckgo.com/lite/";
const USER_AGENT: &str = "Mozilla/5.0 (compatible; tiny-agent/0.1)";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let body = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build http client")?
        .post(ENDPOINT)
        .header("User-Agent", USER_AGENT)
        .form(&[("q", query)])
        .send()
        .await
        .context("send web search request")?
        .error_for_status()
        .context("web search returned error status")?
        .text()
        .await
        .context("read web search response")?;
    Ok(parse(&body, limit))
}

fn parse(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut cursor = 0;
    while results.len() < limit {
        let Some(link_pos) = find_from(html, "result-link", cursor) else {
            break;
        };
        let Some(tag_start) = html[..link_pos].rfind("<a ") else {
            cursor = link_pos + 1;
            continue;
        };
        let tag_end = match html[tag_start..].find('>') {
            Some(i) => tag_start + i,
            None => break,
        };
        let Some(url) = extract_attr(&html[tag_start..tag_end], "href") else {
            cursor = tag_end + 1;
            continue;
        };
        let body_end = match html[tag_end..].find("</a>") {
            Some(i) => tag_end + i,
            None => break,
        };
        let title = clean_text(&html[tag_end + 1..body_end]);
        let snippet = extract_snippet(html, body_end).unwrap_or_default();

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
        cursor = body_end + 4;
    }
    results
}

fn extract_snippet(html: &str, after: usize) -> Option<String> {
    let marker = find_from(html, "result-snippet", after)?;
    let open_end = after + html[marker..].find('>')?;
    let close = open_end + html[open_end..].find("</td>")?;
    Some(clean_text(&html[open_end + 1..close]))
}

fn find_from(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    haystack[from..].find(needle).map(|i| from + i)
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=");
    let start = tag.find(&key)? + key.len();
    let rest = &tag[start..];
    let (quote, body) = match rest.as_bytes().first()? {
        b'"' => ('"', &rest[1..]),
        b'\'' => ('\'', &rest[1..]),
        _ => return None,
    };
    let end = body.find(quote)?;
    Some(decode_entities(&body[..end]))
}

fn clean_text(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut in_tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(out.split_whitespace().collect::<Vec<_>>().join(" ").as_str())
}

