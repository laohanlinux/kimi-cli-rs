use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

/// Wraps text in `<system>` tags.
pub fn system(message: impl Into<String>) -> ContentPart {
    let text = message.into();
    ContentPart::Text {
        text: format!("<system>{text}</system>"),
    }
}

/// Wraps text in `<system-reminder>` tags.
pub fn system_reminder(message: impl Into<String>) -> ContentPart {
    let text = message.into();
    ContentPart::Text {
        text: format!("<system-reminder>\n{text}\n</system-reminder>"),
    }
}

/// Checks whether a message is an internal system-reminder user message.
pub fn is_system_reminder_message(message: &Message) -> bool {
    if message.role != "user" || message.content.len() != 1 {
        return false;
    }
    matches!(
        &message.content[0],
        ContentPart::Text { text } if text.trim().starts_with("<system-reminder>")
    )
}

/// Converts a tool result to a tool message.
pub fn tool_result_to_message(tool_result: &ToolResult) -> Message {
    let mut content: Vec<ContentPart> = Vec::new();

    match &tool_result.return_value {
        ToolReturnValue::Error { error } => {
            content.push(system(format!("ERROR: {error}")));
        }
        ToolReturnValue::Ok { output, message } => {
            if let Some(msg) = message {
                content.push(system(msg.clone()));
            }
            if !output.is_empty() {
                content.push(ContentPart::Text { text: output.clone() });
            }
            if content.is_empty() {
                content.push(system("Tool output is empty."));
            } else if !content.iter().any(|p| matches!(p, ContentPart::Text { .. })) {
                content.insert(0, system("Tool returned non-text content."));
            }
        }
        ToolReturnValue::Parts { parts } => {
            content.extend(parts.iter().cloned());
            if content.is_empty() {
                content.push(system("Tool output is empty."));
            } else if !content.iter().any(|p| matches!(p, ContentPart::Text { .. })) {
                content.insert(0, system("Tool returned non-text content."));
            }
        }
    }

    Message {
        role: "tool".into(),
        content,
        tool_calls: None,
        tool_call_id: Some(tool_result.tool_call_id.clone()),
    }
}

/// Validates message content against model capabilities, returning missing ones.
pub fn check_message(message: &Message, model_capabilities: &HashSet<crate::config::ModelCapability>) -> HashSet<crate::config::ModelCapability> {
    let mut needed = HashSet::new();
    for part in &message.content {
        match part {
            ContentPart::ImageUrl { .. } => {
                needed.insert(crate::config::ModelCapability::ImageIn);
            }
            ContentPart::VideoUrl { .. } => {
                needed.insert(crate::config::ModelCapability::VideoIn);
            }
            ContentPart::Think { .. } => {
                needed.insert(crate::config::ModelCapability::Thinking);
            }
            _ => {}
        }
    }
    needed.difference(model_capabilities).cloned().collect()
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
    /// Rich multi-part result (used by MCP tools).
    Parts { parts: Vec<ContentPart> },
}

impl ToolReturnValue {
    /// Returns a plain-text representation of the return value.
    pub fn extract_text(&self) -> String {
        match self {
            ToolReturnValue::Ok { output, .. } => output.clone(),
            ToolReturnValue::Error { error } => format!("Error: {error}"),
            ToolReturnValue::Parts { parts } => {
                parts
                    .iter()
                    .map(|p| match p {
                        ContentPart::Text { text } => text.clone(),
                        ContentPart::Think { thought } => thought.clone(),
                        ContentPart::ImageUrl { url } => format!("[Image: {url}]"),
                        ContentPart::AudioUrl { url } => format!("[Audio: {url}]"),
                        ContentPart::VideoUrl { url } => format!("[Video: {url}]"),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }
}
