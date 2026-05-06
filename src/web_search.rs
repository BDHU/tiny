use std::sync::LazyLock;

use anyhow::{Context, Result};
use scraper::{ElementRef, Html, Selector};

use crate::web::client;

const ENDPOINT: &str = "https://lite.duckduckgo.com/lite/";

static LINK_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a.result-link").expect("valid result link selector"));
static SNIPPET_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("td.result-snippet").expect("valid snippet selector"));

#[derive(Debug)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub async fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let html = client()
        .post(ENDPOINT)
        .form(&[("q", query)])
        .send()
        .await
        .context("send web search request")?
        .error_for_status()
        .context("web search returned error status")?
        .text()
        .await
        .context("read web search response")?;
    Ok(parse(&html, limit))
}

fn parse(html: &str, limit: usize) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    document
        .select(&LINK_SELECTOR)
        .take(limit)
        .map(|a| SearchResult {
            title: element_text(a),
            url: normalize_url(a.value().attr("href").unwrap_or_default()),
            snippet: snippet_after(a),
        })
        .collect()
}

fn normalize_url(href: &str) -> String {
    match href {
        h if h.starts_with("//") => format!("https:{h}"),
        h if h.starts_with('/') => format!("https://duckduckgo.com{h}"),
        h => h.to_string(),
    }
}

fn element_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

fn snippet_after(link: ElementRef<'_>) -> String {
    let Some(row) = link
        .parent()
        .and_then(ElementRef::wrap)
        .and_then(|cell| cell.parent().and_then(ElementRef::wrap))
    else {
        return String::new();
    };

    for node in row.next_siblings() {
        let Some(element) = ElementRef::wrap(node) else {
            continue;
        };
        if element.select(&LINK_SELECTOR).next().is_some() {
            break;
        }
        if let Some(snippet) = element.select(&SNIPPET_SELECTOR).next() {
            return element_text(snippet);
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_duckduckgo_lite_results() {
        let html = r#"
            <table>
              <tr><td><a rel="nofollow" href="https://rust-lang.org/" class='result-link'>Rust &amp; Language</a></td></tr>
              <tr><td class='result-snippet'><b>Rust</b> is fast &amp; reliable.</td></tr>
              <tr><td><a rel="nofollow" href="https://example.com/" class='result-link'>Other</a></td></tr>
            </table>
        "#;

        let results = parse(html, 1);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust & Language");
        assert_eq!(results[0].url, "https://rust-lang.org/");
        assert_eq!(results[0].snippet, "Rust is fast & reliable.");
    }
}
