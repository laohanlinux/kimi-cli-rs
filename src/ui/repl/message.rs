//! REPL transcript line (Claude Code `Message` text view).

use serde::{Deserialize, Serialize};

/// Role in the REPL transcript (ported from Claude `MessageRole`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplMessage {
    pub role: ReplMessageRole,
    pub text: String,
}

impl ReplMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ReplMessageRole::User,
            text: text.into(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ReplMessageRole::Assistant,
            text: text.into(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: ReplMessageRole::System,
            text: text.into(),
        }
    }
}
