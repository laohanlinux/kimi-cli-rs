use serde::{Deserialize, Serialize};

/// A content part within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Think { thought: String },
    ImageUrl { url: String },
    AudioUrl { url: String },
    VideoUrl { url: String },
}

/// A chat message for the LLM context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Extracts all text parts joined by the given separator.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn extract_text(&self, sep: &str) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(sep)
    }
}

/// An LLM-requested tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub return_value: ToolReturnValue,
}

/// Discriminated union for tool return values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolReturnValue {
    Ok { output: String, message: Option<String> },
    Error { error: String },
}
