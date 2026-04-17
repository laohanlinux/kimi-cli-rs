use std::path::Path;

use crate::soul::message::ToolReturnValue;
use crate::tools::display::BriefDisplayBlock;
use serde_json::Value;

/// Default maximum characters for tool output.
pub const DEFAULT_MAX_CHARS: usize = 50_000;
/// Default maximum line length for tool output.
pub const DEFAULT_MAX_LINE_LENGTH: usize = 2_000;

/// Load a tool description from a file, rendered via MiniJinja with `${var}` syntax.
/// Undefined variables are kept as placeholders instead of raising errors.
pub fn load_desc(
    path: &Path,
    context: Option<&serde_json::Map<String, Value>>,
) -> crate::error::Result<String> {
    let description = std::fs::read_to_string(path).map_err(crate::error::KimiCliError::Io)?;

    // Replace ${var} with {{ var }} for MiniJinja compatibility.
    let re = regex::Regex::new(r"\$\{([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
    let template_text = re.replace_all(&description, "{{ $1 }}");

    let env = minijinja::Environment::new();
    let template = env
        .template_from_str(&template_text)
        .map_err(|e| crate::error::KimiCliError::Generic(format!("template parse error: {e}")))?;

    let ctx: std::collections::HashMap<String, String> = context
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or(&v.to_string()).to_string()))
                .collect()
        })
        .unwrap_or_default();

    let rendered = match template.render(ctx) {
        Ok(s) => s,
        Err(e) if e.kind() == minijinja::ErrorKind::UndefinedError => {
            // Fallback: replace known keys and keep unknown placeholders.
            let mut result = description.clone();
            if let Some(ctx_map) = context {
                for (k, v) in ctx_map.iter() {
                    let placeholder = format!("${{{k}}}");
                    let value = v
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string());
                    result = result.replace(&placeholder, &value);
                }
            }
            result
        }
        Err(e) => {
            return Err(crate::error::KimiCliError::Generic(format!(
                "template render error: {e}"
            )));
        }
    };

    Ok(rendered)
}

/// Truncate a line if it exceeds `max_length`, preserving the beginning and the line break.
/// The output may be longer than `max_length` if it is too short to fit the marker.
pub fn truncate_line(line: &str, max_length: usize, marker: &str) -> String {
    if line.len() <= max_length {
        return line.to_string();
    }

    // Find line breaks at the end of the line
    let re = regex::Regex::new(r"[\r\n]+$").unwrap();
    let linebreak = re.find(line).map(|m| m.as_str()).unwrap_or("");
    let end = format!("{marker}{linebreak}");
    let max_length = max_length.max(end.len());
    let take = max_length.saturating_sub(end.len());
    format!("{}{end}", &line[..take])
}

/// Builder for tool results with character and line limits.
#[derive(Debug, Clone)]
pub struct ToolResultBuilder {
    max_chars: usize,
    max_line_length: Option<usize>,
    marker: String,
    buffer: Vec<String>,
    n_chars: usize,
    n_lines: usize,
    truncation_happened: bool,
    display: Vec<Value>,
    extras: Option<serde_json::Map<String, Value>>,
}

impl ToolResultBuilder {
    /// Create a new builder with the given limits.
    pub fn new(max_chars: usize, max_line_length: Option<usize>) -> Self {
        assert!(
            max_line_length.map(|l| l > 14).unwrap_or(true),
            "max_line_length must be greater than len('[...truncated]')"
        );
        Self {
            max_chars,
            max_line_length,
            marker: "[...truncated]".into(),
            buffer: Vec::new(),
            n_chars: 0,
            n_lines: 0,
            truncation_happened: false,
            display: Vec::new(),
            extras: None,
        }
    }

    /// Check if output buffer is full due to character limit.
    pub fn is_full(&self) -> bool {
        self.n_chars >= self.max_chars
    }

    /// Get current character count.
    pub fn n_chars(&self) -> usize {
        self.n_chars
    }

    /// Get current line count.
    pub fn n_lines(&self) -> usize {
        self.n_lines
    }

    /// Write text to the output buffer.
    /// Returns the number of characters actually written.
    pub fn write(&mut self, text: &str) -> usize {
        if self.is_full() || text.is_empty() {
            return 0;
        }

        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut chars_written = 0;
        for line in lines {
            if self.is_full() {
                break;
            }

            let original_line = line;
            let remaining_chars = self.max_chars.saturating_sub(self.n_chars);
            let limit = self
                .max_line_length
                .map(|l| remaining_chars.min(l))
                .unwrap_or(remaining_chars);
            let processed = truncate_line(line, limit, &self.marker);
            if processed != original_line {
                self.truncation_happened = true;
            }

            self.buffer.push(processed.clone());
            chars_written += processed.len();
            self.n_chars += processed.len();
            if processed.ends_with('\n') {
                self.n_lines += 1;
            }
        }

        chars_written
    }

    /// Add display blocks to the tool result.
    pub fn display(&mut self, blocks: &[Value]) {
        self.display.extend_from_slice(blocks);
    }

    /// Add a single display block.
    pub fn display_one(&mut self, block: Value) {
        self.display.push(block);
    }

    /// Add extra data to the tool result.
    pub fn extras(&mut self, extras: serde_json::Map<String, Value>) {
        if let Some(ref mut existing) = self.extras {
            for (k, v) in extras {
                existing.insert(k, v);
            }
        } else {
            self.extras = Some(extras);
        }
    }

    /// Take the accumulated display blocks.
    pub fn take_display(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.display)
    }

    /// Take the accumulated extras.
    pub fn take_extras(&mut self) -> Option<Value> {
        self.extras.take().map(Value::Object)
    }

    /// Create a `ToolReturnValue::Ok` with the current output.
    pub fn ok(mut self, message: &str, brief: Option<&str>) -> ToolReturnValue {
        let output = self.buffer.concat();
        let mut final_message = message.to_string();
        if !final_message.is_empty() && !final_message.ends_with('.') {
            final_message.push('.');
        }
        let truncation_msg = "Output is truncated to fit in the message.";
        if self.truncation_happened {
            if !final_message.is_empty() {
                final_message.push(' ');
                final_message.push_str(truncation_msg);
            } else {
                final_message = truncation_msg.into();
            }
        }
        if let Some(b) = brief {
            let block = BriefDisplayBlock::new(b);
            self.display.insert(0, serde_json::to_value(block).unwrap());
        }
        ToolReturnValue::Ok {
            output,
            message: if final_message.is_empty() {
                None
            } else {
                Some(final_message)
            },
        }
    }

    /// Create a `ToolReturnValue::Error` with the current output.
    pub fn error(mut self, message: &str, brief: &str) -> ToolReturnValue {
        let output = self.buffer.concat();
        let mut final_message = message.to_string();
        if self.truncation_happened {
            let truncation_msg = "Output is truncated to fit in the message.";
            if !final_message.is_empty() {
                final_message.push(' ');
                final_message.push_str(truncation_msg);
            } else {
                final_message = truncation_msg.into();
            }
        }
        let block = BriefDisplayBlock::new(brief);
        self.display.insert(0, serde_json::to_value(block).unwrap());
        ToolReturnValue::Error {
            error: if final_message.is_empty() {
                output.clone()
            } else {
                final_message
            },
        }
    }
}

impl Default for ToolResultBuilder {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CHARS, Some(DEFAULT_MAX_LINE_LENGTH))
    }
}

/// Error type for a tool call rejected by the user.
#[derive(Debug, Clone)]
pub struct ToolRejectedError {
    pub message: String,
    pub brief: String,
    pub has_feedback: bool,
}

impl ToolRejectedError {
    pub fn new(message: Option<&str>, brief: &str, has_feedback: bool) -> Self {
        Self {
            message: message.unwrap_or(
                "The tool call is rejected by the user. Stop what you are doing and wait for the user to tell you how to proceed."
            ).into(),
            brief: brief.into(),
            has_feedback,
        }
    }
}

impl std::fmt::Display for ToolRejectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (brief: {})", self.message, self.brief)
    }
}

impl std::error::Error for ToolRejectedError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_builder_default_limits() {
        let mut b = ToolResultBuilder::default();
        assert!(!b.is_full());
        assert_eq!(b.write("hello\nworld"), 11);
        assert_eq!(b.n_chars(), 11);
        assert_eq!(b.n_lines(), 1);
    }

    #[test]
    fn tool_result_builder_truncates_lines() {
        let mut b = ToolResultBuilder::new(100, Some(20));
        let long_line = "a".repeat(50);
        let written = b.write(&long_line);
        assert!(written < 50);
        assert!(b.truncation_happened);
    }

    #[test]
    fn tool_result_builder_respects_max_chars() {
        let mut b = ToolResultBuilder::new(10, None);
        // The marker is 14 chars, so truncate_line expands the limit to 14.
        assert_eq!(b.write("hello world this is long"), 14);
        assert!(b.is_full());
        assert_eq!(b.write("more"), 0);
    }

    #[test]
    fn tool_result_builder_ok_with_truncation_message() {
        let mut b = ToolResultBuilder::new(10, None);
        b.write("hello world this is long");
        let result = b.ok("Done", None);
        match result {
            ToolReturnValue::Ok {
                message: Some(msg), ..
            } => {
                assert!(msg.contains("truncated"));
            }
            _ => panic!("expected Ok with message"),
        }
    }

    #[test]
    fn tool_result_builder_error_includes_brief_display() {
        let b = ToolResultBuilder::default();
        let result = b.error("It broke", "Brief");
        match result {
            ToolReturnValue::Error { error } => {
                assert!(error.contains("It broke"));
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn truncate_line_basic() {
        assert_eq!(truncate_line("short", 10, "..."), "short");
        let s = "a".repeat(100);
        let out = truncate_line(&s, 10, "...");
        assert!(out.ends_with("..."));
        assert_eq!(out.len(), 10); // 7 + 3, limited to max_length
    }

    #[test]
    fn truncate_line_preserves_newline() {
        let s = format!("{}\n", "a".repeat(100));
        let out = truncate_line(&s, 10, "...");
        assert!(out.ends_with("...\n"));
    }

    #[test]
    fn tool_rejected_error_display() {
        let e = ToolRejectedError::new(None, "Rejected by user", false);
        assert!(e.to_string().contains("Rejected by user"));
    }
}
