use rmcp::model::{CallToolResult, RawContent};

/// Maximum characters allowed in MCP tool output before truncation.
const MCP_MAX_OUTPUT_CHARS: usize = 100_000;

/// Converts an rmcp `CallToolResult` into a Kimi `ToolReturnValue`.
pub fn convert_mcp_result(result: &CallToolResult) -> crate::soul::message::ToolReturnValue {
    let mut content: Vec<crate::soul::message::ContentPart> = Vec::new();
    let mut char_budget = MCP_MAX_OUTPUT_CHARS;
    let mut truncated = false;

    for part in &result.content {
        let converted = match &part.raw {
            RawContent::Text(text_content) => Some(crate::soul::message::ContentPart::Text {
                text: text_content.text.clone(),
            }),
            RawContent::Image(image_content) => {
                // Build a data URL for the image.
                let url = format!(
                    "data:{};base64,{}",
                    image_content.mime_type, image_content.data
                );
                Some(crate::soul::message::ContentPart::ImageUrl { url })
            }
            RawContent::Audio(audio_content) => {
                let url = format!(
                    "data:{};base64,{}",
                    audio_content.mime_type, audio_content.data
                );
                Some(crate::soul::message::ContentPart::AudioUrl { url })
            }
            RawContent::Resource(resource) => {
                // For embedded resources, try to extract text; otherwise skip.
                let text = match &resource.resource {
                    rmcp::model::ResourceContents::TextResourceContents { text, .. } => {
                        text.clone()
                    }
                    rmcp::model::ResourceContents::BlobResourceContents { blob, .. } => {
                        format!("[Binary resource: {} bytes]", blob.len())
                    }
                };
                Some(crate::soul::message::ContentPart::Text { text })
            }
            RawContent::ResourceLink(link) => Some(crate::soul::message::ContentPart::Text {
                text: format!("[Resource link: {}]", link.uri),
            }),
        };

        let Some(converted) = converted else { continue };

        // Budget enforcement.
        match &converted {
            crate::soul::message::ContentPart::Text { text } => {
                if char_budget == 0 {
                    truncated = true;
                    continue;
                }
                let mut text = text.clone();
                if text.len() > char_budget {
                    text.truncate(char_budget);
                    truncated = true;
                }
                char_budget -= text.len();
                content.push(crate::soul::message::ContentPart::Text { text });
            }
            crate::soul::message::ContentPart::ImageUrl { url }
            | crate::soul::message::ContentPart::AudioUrl { url }
            | crate::soul::message::ContentPart::VideoUrl { url } => {
                let size = url.len();
                if size > char_budget {
                    truncated = true;
                    continue;
                }
                char_budget -= size;
                content.push(converted);
            }
            _ => {
                content.push(converted);
            }
        }
    }

    if truncated {
        content.push(crate::soul::message::ContentPart::Text {
            text: format!(
                "\n\n[Output truncated: exceeded {MCP_MAX_OUTPUT_CHARS} character limit. \
                 Use pagination or more specific queries to get remaining content.]"
            ),
        });
    }

    let is_error = result.is_error.unwrap_or(false);
    if is_error {
        crate::soul::message::ToolReturnValue::Error {
            error:
                "Tool returned an error. The output may be an error message or incomplete output."
                    .into(),
        }
    } else {
        crate::soul::message::ToolReturnValue::Parts { parts: content }
    }
}
