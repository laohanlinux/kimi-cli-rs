use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use crate::soul::message::Message;

/// Persistent conversation context backed by a JSONL file.
pub struct Context {
    history: Vec<Message>,
    token_count: usize,
    pending_token_estimate: usize,
    next_checkpoint_id: usize,
    system_prompt: Option<String>,
    file_backend: PathBuf,
    restored: bool,
}

impl Context {
    /// Creates a new context manager for the given file path.
    pub fn new(file_backend: PathBuf) -> Self {
        Self {
            history: Vec::new(),
            token_count: 0,
            pending_token_estimate: 0,
            next_checkpoint_id: 0,
            system_prompt: None,
            file_backend,
            restored: false,
        }
    }

    /// Restores the context from the JSONL file.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn restore(&mut self) -> crate::error::Result<()> {
        if self.restored && !self.history.is_empty() {
            return Err(crate::error::KimiCliError::Generic(
                "Context has already been modified; restore is not allowed.".into(),
            ));
        }
        self.history.clear();
        self.token_count = 0;
        self.pending_token_estimate = 0;
        self.next_checkpoint_id = 0;
        self.system_prompt = None;

        if !self.file_backend.exists() {
            self.restored = true;
            return Ok(());
        }
        let text = match tokio::fs::read_to_string(&self.file_backend).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("failed to read context file: {}", e);
                self.restored = true;
                return Ok(());
            }
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Err(e) = self.apply_record(line) {
                tracing::warn!("skipping malformed context line: {e}");
            }
        }
        self.restored = true;
        Ok(())
    }

    fn apply_record(&mut self, line: &str) -> crate::error::Result<()> {
        let record: serde_json::Value = serde_json::from_str(line)?;
        let role = record.get("role").and_then(|v| v.as_str());
        match role {
            Some("_system_prompt") => {
                self.system_prompt = record.get("content").and_then(|v| v.as_str()).map(String::from);
            }
            Some("_usage") => {
                self.token_count = record.get("token_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.pending_token_estimate = 0;
            }
            Some("_checkpoint") => {
                self.next_checkpoint_id = record.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize + 1;
            }
            _ => {
                let msg: Message = serde_json::from_value(record)?;
                self.history.push(msg);
            }
        }
        Ok(())
    }

    /// Appends a message to memory and the JSONL file.
    #[tracing::instrument(level = "trace", skip(self, message))]
    pub async fn append_message(&mut self, message: &Message) -> crate::error::Result<()> {
        self.pending_token_estimate += estimate_text_tokens(&message.extract_text(" "));
        self.history.push(message.clone());
        let line = serde_json::to_string(message)?;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Appends multiple messages in a batch.
    pub async fn append_messages(&mut self, messages: &[Message]) -> crate::error::Result<()> {
        for msg in messages {
            self.append_message(msg).await?;
        }
        Ok(())
    }

    /// Persists the current token count as a usage marker.
    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn update_token_count(&mut self, count: usize) -> crate::error::Result<()> {
        self.token_count = count;
        self.pending_token_estimate = 0;
        let record = serde_json::json!({"role": "_usage", "token_count": count});
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(serde_json::to_string(&record)?.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Creates a checkpoint marker in the file, optionally with a user message.
    #[tracing::instrument(level = "debug", skip(self, add_user_message))]
    pub async fn checkpoint(
        &mut self,
        add_user_message: Option<&Message>,
    ) -> crate::error::Result<()> {
        if let Some(msg) = add_user_message {
            self.append_message(msg).await?;
        }
        let id = self.next_checkpoint_id;
        self.next_checkpoint_id += 1;
        let record = serde_json::json!({"role": "_checkpoint", "id": id});
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(serde_json::to_string(&record)?.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Returns the full message history.
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Returns the current system prompt, if any.
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// Sets the system prompt and persists it, prepending atomically if file exists.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn write_system_prompt(&mut self, prompt: &str) -> crate::error::Result<()> {
        self.system_prompt = Some(prompt.to_string());
        let record = serde_json::json!({"role": "_system_prompt", "content": prompt});
        let line = serde_json::to_string(&record)?;

        if self.file_backend.exists() {
            let existing = tokio::fs::read_to_string(&self.file_backend).await.unwrap_or_default();
            let tmp = self.file_backend.with_extension("tmp");
            let mut file = tokio::fs::File::create(&tmp).await?;
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
            file.write_all(existing.as_bytes()).await?;
            tokio::fs::rename(&tmp, &self.file_backend).await?;
        } else {
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(&self.file_backend)
                .await?;
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }
        Ok(())
    }

    /// Returns the next checkpoint ID that will be assigned.
    pub fn next_checkpoint_id(&self) -> usize {
        self.next_checkpoint_id
    }

    /// Rotates the context file to the next available backup name.
    fn next_available_rotation(&self) -> Option<PathBuf> {
        for i in 1..=1000 {
            let candidate = self.file_backend.with_extension(format!("jsonl.{i}"));
            if !candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// Reverts the context file and in-memory state to the given checkpoint.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn revert_to_checkpoint(&mut self, checkpoint_id: usize) -> crate::error::Result<()> {
        if !self.file_backend.exists() {
            return Ok(());
        }
        let text = tokio::fs::read_to_string(&self.file_backend).await?;
        let mut kept = Vec::new();
        let mut found = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            kept.push(trimmed);
            if let Ok(record) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if record.get("role").and_then(|v| v.as_str()) == Some("_checkpoint") {
                    if record.get("id").and_then(|v| v.as_u64()).map(|id| id as usize)
                        == Some(checkpoint_id)
                    {
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            tracing::warn!("checkpoint {} not found, no revert performed", checkpoint_id);
            return Ok(());
        }
        if let Some(rotated) = self.next_available_rotation() {
            tokio::fs::rename(&self.file_backend, &rotated).await?;
        }
        let truncated = kept.join("\n") + "\n";
        tokio::fs::write(&self.file_backend, truncated).await?;
        self.history.clear();
        self.token_count = 0;
        self.pending_token_estimate = 0;
        self.next_checkpoint_id = 0;
        self.system_prompt = None;
        self.restored = false;
        self.restore().await?;
        tracing::info!("reverted context to checkpoint {}", checkpoint_id);
        Ok(())
    }

    /// Clears the context by rotating the file and resetting state.
    pub async fn clear(&mut self) -> crate::error::Result<()> {
        if let Some(rotated) = self.next_available_rotation() {
            if self.file_backend.exists() {
                tokio::fs::rename(&self.file_backend, &rotated).await?;
            }
        }
        self.history.clear();
        self.token_count = 0;
        self.pending_token_estimate = 0;
        self.next_checkpoint_id = 0;
        self.system_prompt = None;
        self.restored = false;
        Ok(())
    }
}

/// Naive token estimator for English text (~4 chars/token).
fn estimate_text_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_increments_id() {
        let tmp = std::env::temp_dir().join(format!("kimi-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut ctx = Context::new(tmp.join("context.jsonl"));
        assert_eq!(ctx.next_checkpoint_id(), 0);
        ctx.checkpoint(None).await.unwrap();
        assert_eq!(ctx.next_checkpoint_id(), 1);
        ctx.checkpoint(None).await.unwrap();
        assert_eq!(ctx.next_checkpoint_id(), 2);
    }

    #[tokio::test]
    async fn revert_to_checkpoint_truncates_file() {
        let tmp = std::env::temp_dir().join(format!("kimi-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("context.jsonl");
        let mut ctx = Context::new(path.clone());

        let msg1 = Message {
            role: "user".into(),
            content: vec![crate::soul::message::ContentPart::Text { text: "hello".into() }],
            tool_calls: None,
            tool_call_id: None,
        };
        ctx.append_message(&msg1).await.unwrap();
        ctx.checkpoint(None).await.unwrap();

        let msg2 = Message {
            role: "assistant".into(),
            content: vec![crate::soul::message::ContentPart::Text { text: "world".into() }],
            tool_calls: None,
            tool_call_id: None,
        };
        ctx.append_message(&msg2).await.unwrap();
        ctx.checkpoint(None).await.unwrap();

        let msg3 = Message {
            role: "user".into(),
            content: vec![crate::soul::message::ContentPart::Text { text: "after".into() }],
            tool_calls: None,
            tool_call_id: None,
        };
        ctx.append_message(&msg3).await.unwrap();

        assert_eq!(ctx.history().len(), 3);
        assert_eq!(ctx.next_checkpoint_id(), 2);

        ctx.revert_to_checkpoint(0).await.unwrap();

        // After revert to checkpoint 0, only msg1 and the first checkpoint should remain.
        assert_eq!(ctx.history().len(), 1);
        assert_eq!(ctx.next_checkpoint_id(), 1);
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(text.contains("hello"));
        assert!(!text.contains("world"));
        assert!(!text.contains("after"));
    }

    #[tokio::test]
    async fn revert_to_missing_checkpoint_is_noop() {
        let tmp = std::env::temp_dir().join(format!("kimi-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut ctx = Context::new(tmp.join("context.jsonl"));

        let msg = Message {
            role: "user".into(),
            content: vec![crate::soul::message::ContentPart::Text { text: "hi".into() }],
            tool_calls: None,
            tool_call_id: None,
        };
        ctx.append_message(&msg).await.unwrap();

        ctx.revert_to_checkpoint(99).await.unwrap();
        assert_eq!(ctx.history().len(), 1);
    }
}
