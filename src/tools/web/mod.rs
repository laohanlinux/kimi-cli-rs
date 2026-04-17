use async_trait::async_trait;
use secrecy::ExposeSecret;

/// Searches the web.
#[derive(Debug, Clone, Default)]
pub struct SearchWeb;

#[async_trait]
impl crate::soul::toolset::Tool for SearchWeb {
    fn name(&self) -> &str {
        "SearchWeb"
    }

    fn description(&self) -> &str {
        "Search the web for the given query."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "limit": { "type": "integer", "default": 5, "description": "Number of results to return" },
                "include_content": { "type": "boolean", "default": false, "description": "Include page content in results" }
            },
            "required": ["query"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let query = match arguments.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.is_empty() => q,
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Query cannot be empty.".into(),
                };
            }
        };
        let limit = arguments.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let include_content = arguments.get("include_content").and_then(|v| v.as_bool()).unwrap_or(false);

        if let Some(ref config) = runtime.config.services.moonshot_search {
            return search_moonshot(query, limit, include_content, config, runtime).await;
        }

        search_duckduckgo(query, limit).await
    }
}

/// Fetches a URL.
#[derive(Debug, Clone, Default)]
pub struct FetchUrl;

#[async_trait]
impl crate::soul::toolset::Tool for FetchUrl {
    fn name(&self) -> &str {
        "FetchURL"
    }

    fn description(&self) -> &str {
        "Fetch the contents of a URL."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" }
            },
            "required": ["url"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let url = match arguments.get("url").and_then(|v| v.as_str()) {
            Some(u) if !u.is_empty() => u,
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "URL cannot be empty.".into(),
                };
            }
        };

        if let Some(ref config) = runtime.config.services.moonshot_fetch {
            let result = fetch_moonshot(url, config, runtime).await;
            if !matches!(result, crate::soul::message::ToolReturnValue::Error { .. }) {
                return result;
            }
            tracing::warn!("moonshot fetch failed, falling back to direct HTTP GET");
        }

        fetch_with_http_get(url).await
    }
}

async fn search_moonshot(
    query: &str,
    limit: usize,
    include_content: bool,
    config: &crate::config::MoonshotSearchConfig,
    runtime: &crate::soul::agent::Runtime,
) -> crate::soul::message::ToolReturnValue {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to build HTTP client: {e}"),
            };
        }
    };

    let mut headers = reqwest::header::HeaderMap::new();
    let _ = headers.insert("User-Agent", crate::constant::USER_AGENT.as_str().parse().unwrap());
    let api_key = match runtime.oauth.resolve_api_key(&config.api_key, config.oauth.as_ref()).await {
        Some(k) => k,
        None => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Search service is not configured. Set up moonshot_search in config to enable this tool.".into(),
            };
        }
    };
    let _ = headers.insert(
        "Authorization",
        format!("Bearer {}", api_key.expose_secret()).parse().unwrap(),
    );
    for (k, v) in runtime.oauth.common_headers() {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
            let _ = headers.insert(name, v.parse().unwrap());
        }
    }
    if let Some(ref tool_call) = crate::soul::toolset::get_current_tool_call_or_none() {
        let _ = headers.insert("X-Msh-Tool-Call-Id", tool_call.id.parse().unwrap());
    }
    if let Some(ref custom) = config.custom_headers {
        for (k, v) in custom {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                let _ = headers.insert(name, v.parse().unwrap());
            }
        }
    }

    let body = serde_json::json!({
        "text_query": query,
        "limit": limit,
        "enable_page_crawling": include_content,
        "timeout_seconds": 30,
    });

    let response = match client.post(&config.base_url).headers(headers).json(&body).send().await {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Search request timed out.".into(),
            };
        }
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Search request failed: {e}"),
            };
        }
    };

    let status = response.status();
    if status.as_u16() >= 400 {
        return crate::soul::message::ToolReturnValue::Error {
            error: format!("Search service returned HTTP {status}."),
        };
    }

    let json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to parse search response: {e}"),
            };
        }
    };

    let results = json.get("search_results").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if results.is_empty() {
        return crate::soul::message::ToolReturnValue::Ok {
            output: "No search results found.".into(),
            message: None,
        };
    }

    let mut lines = Vec::new();
    for (i, result) in results.iter().take(limit).enumerate() {
        if i > 0 {
            lines.push("---".into());
            lines.push(String::new());
        }
        let title = result.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let url = result.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let snippet = result.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
        let date = result.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let content = result.get("content").and_then(|v| v.as_str()).unwrap_or("");
        lines.push(format!("Title: {title}"));
        if !date.is_empty() {
            lines.push(format!("Date: {date}"));
        }
        lines.push(format!("URL: {url}"));
        lines.push(format!("Summary: {snippet}"));
        if !content.is_empty() {
            lines.push(String::new());
            lines.push(content.into());
        }
    }

    crate::soul::message::ToolReturnValue::Ok {
        output: lines.join("\n"),
        message: Some(format!("Found {} search results.", results.len().min(limit))),
    }
}

async fn search_duckduckgo(query: &str, limit: usize) -> crate::soul::message::ToolReturnValue {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to build HTTP client: {e}"),
            };
        }
    };

    let url = format!("https://lite.duckduckgo.com/lite/?q={}", query.replace(' ', "+"));
    let response = match client
        .get(&url)
        .header("User-Agent", crate::constant::USER_AGENT.as_str())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Search request timed out.".into(),
            };
        }
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Search request failed: {e}"),
            };
        }
    };

    let status = response.status();
    if status.as_u16() >= 400 {
        return crate::soul::message::ToolReturnValue::Error {
            error: format!("Search service returned HTTP {status}."),
        };
    }

    let html = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to read search response: {e}"),
            };
        }
    };

    let results = parse_duckduckgo_results(&html);
    if results.is_empty() {
        return crate::soul::message::ToolReturnValue::Ok {
            output: "No search results found.".into(),
            message: None,
        };
    }

    let mut lines = Vec::new();
    for (i, result) in results.iter().take(limit).enumerate() {
        if i > 0 {
            lines.push("---".into());
            lines.push(String::new());
        }
        lines.push(format!("Title: {}", result.title));
        lines.push(format!("URL: {}", result.url));
        lines.push(format!("Summary: {}", result.snippet));
    }

    crate::soul::message::ToolReturnValue::Ok {
        output: lines.join("\n"),
        message: Some(format!("Found {} search results via DuckDuckGo.", results.len().min(limit))),
    }
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

fn parse_duckduckgo_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    // DuckDuckGo Lite uses table rows with result links and snippets.
    // This is a best-effort regex-based parser.
    let link_re = regex::Regex::new(r#"<a[^>]+class=""[^>]*href="([^"]+)"[^>]*>([^<]+)</a>"#).unwrap();
    let snippet_re = regex::Regex::new(r#"<td[^>]*class="result-snippet"[^>]*>(.*?)</td>"#).unwrap();

    let links: Vec<(String, String)> = link_re
        .captures_iter(html)
        .filter_map(|c| {
            let url = c.get(1)?.as_str().to_string();
            let title = c.get(2)?.as_str().trim().to_string();
            if url.starts_with("http") && !title.is_empty() {
                Some((url, title))
            } else {
                None
            }
        })
        .collect();

    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .filter_map(|c| {
            let snippet = c.get(1)?.as_str();
            Some(strip_html_tags(snippet).trim().to_string())
        })
        .collect();

    for (i, (url, title)) in links.iter().enumerate() {
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        results.push(SearchResult {
            title: title.clone(),
            url: url.clone(),
            snippet,
        });
    }

    results
}

async fn fetch_moonshot(
    url: &str,
    config: &crate::config::MoonshotFetchConfig,
    runtime: &crate::soul::agent::Runtime,
) -> crate::soul::message::ToolReturnValue {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to build HTTP client: {e}"),
            };
        }
    };

    let api_key = match runtime.oauth.resolve_api_key(&config.api_key, config.oauth.as_ref()).await {
        Some(k) => k,
        None => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Fetch service is not configured. Set up moonshot_fetch in config to enable this tool.".into(),
            };
        }
    };

    let mut headers = reqwest::header::HeaderMap::new();
    let _ = headers.insert("User-Agent", crate::constant::USER_AGENT.as_str().parse().unwrap());
    let _ = headers.insert(
        "Authorization",
        format!("Bearer {}", api_key.expose_secret()).parse().unwrap(),
    );
    for (k, v) in runtime.oauth.common_headers() {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
            let _ = headers.insert(name, v.parse().unwrap());
        }
    }
    let _ = headers.insert("Accept", "text/markdown".parse().unwrap());
    if let Some(ref tool_call) = crate::soul::toolset::get_current_tool_call_or_none() {
        let _ = headers.insert("X-Msh-Tool-Call-Id", tool_call.id.parse().unwrap());
    }
    if let Some(ref custom) = config.custom_headers {
        for (k, v) in custom {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                let _ = headers.insert(name, v.parse().unwrap());
            }
        }
    }

    let body = serde_json::json!({ "url": url });

    let response = match client.post(&config.base_url).headers(headers).json(&body).send().await {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Fetch request timed out.".into(),
            };
        }
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Fetch request failed: {e}"),
            };
        }
    };

    let status = response.status();
    if status.as_u16() >= 400 {
        return crate::soul::message::ToolReturnValue::Error {
            error: format!("Fetch service returned HTTP {status}."),
        };
    }

    let text = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to read fetch response: {e}"),
            };
        }
    };

    crate::soul::message::ToolReturnValue::Ok {
        output: text,
        message: Some("The returned content is the main content extracted from the page.".into()),
    }
}

async fn fetch_with_http_get(url: &str) -> crate::soul::message::ToolReturnValue {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to build HTTP client: {e}"),
            };
        }
    };

    let response = match client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
        )
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Request timed out. The server may be slow or unreachable.".into(),
            };
        }
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Network error: {e}"),
            };
        }
    };

    let status = response.status();
    if status.as_u16() >= 400 {
        return crate::soul::message::ToolReturnValue::Error {
            error: format!("HTTP {status} error. The page may not be accessible."),
        };
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let body = match response.text().await {
        Ok(t) => t,
        Err(e) => {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to read response body: {e}"),
            };
        }
    };

    if body.is_empty() {
        return crate::soul::message::ToolReturnValue::Ok {
            output: "The response body is empty.".into(),
            message: Some("Empty response body".into()),
        };
    }

    if content_type.starts_with("text/plain") || content_type.starts_with("text/markdown") {
        return crate::soul::message::ToolReturnValue::Ok {
            output: body,
            message: Some("The returned content is the full content of the page.".into()),
        };
    }

    // For HTML, perform basic content extraction.
    let extracted = extract_html_text(&body);
    if extracted.is_empty() {
        return crate::soul::message::ToolReturnValue::Ok {
            output: body,
            message: Some("The returned content is the raw page body. HTML extraction yielded no text.".into()),
        };
    }

    crate::soul::message::ToolReturnValue::Ok {
        output: extracted,
        message: Some("The returned content is the extracted text from the page.".into()),
    }
}

/// Semantic HTML text extraction using `readability-rust` (Mozilla Readability port).
/// Falls back to basic regex-based tag stripping when readability yields no content.
fn extract_html_text(html: &str) -> String {
    // Try readability first for article-quality extraction.
    match readability_rust::Readability::new(html, None) {
        Ok(mut readability) => {
            if let Some(article) = readability.parse() {
                if let Some(text) = article.text_content {
                    let text = readability_rust::normalize_whitespace(&text);
                    if !text.is_empty() {
                        return truncate_text(text);
                    }
                }
            }
        }
        Err(e) => {
            tracing::debug!("Readability extraction failed: {}", e);
        }
    }

    // Fallback: basic regex-based extraction.
    let mut text = html.to_string();

    // Remove common non-content blocks.
    for tag in ["script", "style", "nav", "header", "footer", "aside"] {
        let re = regex::Regex::new(&format!(r#"(?is)<{tag}\b[^>]*>.*?</{tag}>"#)).unwrap();
        text = re.replace_all(&text, " ").to_string();
    }

    // Strip remaining tags.
    let tag_re = regex::Regex::new(r#"(?is)<[^>]+>"#).unwrap();
    text = tag_re.replace_all(&text, " ").to_string();

    // Decode common HTML entities.
    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&amp;", "&");
    text = text.replace("&quot;", "\"");
    text = text.replace("&#39;", "'");
    text = text.replace("&nbsp;", " ");

    // Collapse whitespace.
    let ws_re = regex::Regex::new(r#"\s+"#).unwrap();
    text = ws_re.replace_all(&text, " ").trim().to_string();

    truncate_text(text)
}

/// Truncates text to a reasonable length for LLM context.
fn truncate_text(mut text: String) -> String {
    const MAX_CHARS: usize = 100_000;
    if text.len() > MAX_CHARS {
        text.truncate(MAX_CHARS);
        text.push_str("\n\n[Content truncated due to length.]");
    }
    text
}

/// Strips HTML tags from a snippet, preserving text content.
fn strip_html_tags(html: &str) -> String {
    let tag_re = regex::Regex::new(r#"(?is)<[^>]+>"#).unwrap();
    let text = tag_re.replace_all(html, " ").to_string();
    let ws_re = regex::Regex::new(r#"\s+"#).unwrap();
    ws_re.replace_all(&text, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soul::toolset::Tool;

    #[tokio::test]
    async fn fetch_url_empty() {
        let tool = FetchUrl::default();
        let rt = crate::soul::agent::Runtime::default();
        let result = tool.call(serde_json::json!({"url": ""}), &rt).await;
        assert!(matches!(result, crate::soul::message::ToolReturnValue::Error { .. }));
    }

    #[tokio::test]
    async fn search_web_empty_query() {
        let tool = SearchWeb::default();
        let rt = crate::soul::agent::Runtime::default();
        let result = tool.call(serde_json::json!({"query": ""}), &rt).await;
        assert!(matches!(result, crate::soul::message::ToolReturnValue::Error { .. }));
    }

    #[test]
    fn html_extraction_removes_scripts_and_tags() {
        let html = r#"
            <html>
            <head><title>Test</title><script>alert('x')</script></head>
            <body>
            <nav><a href="/">menu</a></nav>
            <main>
            <h1>Welcome</h1>
            <p>Hello &amp; welcome!</p>
            <p>This is a paragraph with enough text to be considered content.</p>
            </main>
            <style>.x{color:red}</style>
            <footer>bye</footer>
            </body>
            </html>
        "#;
        let text = extract_html_text(html);
        assert!(!text.contains("alert"));
        assert!(!text.contains("menu"));
        assert!(!text.contains("bye"));
        assert!(!text.contains("<p>"));
        assert!(text.contains("Hello & welcome!"));
    }

    #[test]
    fn duckduckgo_parser_finds_results() {
        let html = r#"
            <a class="" href="https://example.com">Example</a>
            <td class="result-snippet">A great <b>example</b> site.</td>
            <a class="" href="https://rust-lang.org">Rust</a>
            <td class="result-snippet">The Rust programming language.</td>
        "#;
        let results = parse_duckduckgo_results(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet, "A great example site.");
    }
}
