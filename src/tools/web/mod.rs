use async_trait::async_trait;

pub mod fetch;
pub mod search;

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
        _arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        crate::soul::message::ToolReturnValue::Error {
            error: "Web search service is not configured. Set up moonshot_search in config to enable this tool.".into(),
        }
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
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let url = match arguments.get("url").and_then(|v| v.as_str()) {
            Some(u) if !u.is_empty() => u,
            _ => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "URL cannot be empty.".into(),
                };
            }
        };

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

        // For HTML, return the raw body. Full content extraction (e.g. trafilatura)
        // is not yet implemented in the Rust port.
        crate::soul::message::ToolReturnValue::Ok {
            output: body,
            message: Some(
                "The returned content is the raw page body. HTML extraction is basic.".into(),
            ),
        }
    }
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
    async fn search_web_unconfigured() {
        let tool = SearchWeb::default();
        let rt = crate::soul::agent::Runtime::default();
        let result = tool.call(serde_json::json!({"query": "rust"}), &rt).await;
        assert!(matches!(result, crate::soul::message::ToolReturnValue::Error { .. }));
    }
}
