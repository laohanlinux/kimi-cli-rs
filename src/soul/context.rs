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
        }
    }

    /// Restores the context from the JSONL file.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn restore(&mut self) -> crate::error::Result<()> {
        if !self.file_backend.exists() {
            return Ok(());
        }
        let text = tokio::fs::read_to_string(&self.file_backend).await?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.apply_record(line)?;
        }
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

    /// Creates a checkpoint marker in the file.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn checkpoint(&mut self) -> crate::error::Result<()> {
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

    /// Sets the system prompt and persists it.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn write_system_prompt(&mut self, prompt: &str) -> crate::error::Result<()> {
        self.system_prompt = Some(prompt.to_string());
        let record = serde_json::json!({"role": "_system_prompt", "content": prompt});
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(serde_json::to_string(&record)?.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}

/// Naive token estimator for English text (~4 chars/token).
fn estimate_text_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}
