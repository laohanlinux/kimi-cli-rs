/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<crate::soul::message::Message>,
    pub usage: Option<TokenUsage>,
}

impl CompactionResult {
    /// Estimates the token count of the compacted messages.
    pub fn estimated_token_count(&self) -> usize {
        if self.usage.is_some() && !self.messages.is_empty() {
            let summary_tokens = self.usage.as_ref().unwrap().output;
            let preserved_tokens = estimate_text_tokens(&self.messages[1..]);
            summary_tokens + preserved_tokens
        } else {
            estimate_text_tokens(&self.messages)
        }
    }
}

/// Token usage summary from an LLM call.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub input: usize,
    pub output: usize,
}

/// Estimates tokens from message text content using a character-based heuristic.
pub fn estimate_text_tokens(messages: &[crate::soul::message::Message]) -> usize {
    let total_chars: usize = messages
        .iter()
        .flat_map(|msg| msg.content.iter())
        .filter_map(|part| match part {
            crate::soul::message::ContentPart::Text { text } => Some(text.len()),
            _ => None,
        })
        .sum();
    total_chars.div_ceil(4)
}

/// Determines whether auto-compaction should be triggered.
pub fn should_auto_compact(
    token_count: usize,
    max_context_size: usize,
    trigger_ratio: f64,
    reserved_context_size: usize,
) -> bool {
    token_count >= (max_context_size as f64 * trigger_ratio) as usize
        || token_count + reserved_context_size >= max_context_size
}

/// Protocol for context compaction strategies.
#[async_trait::async_trait]
pub trait Compaction: Send + Sync {
    /// Compacts a sequence of messages into a new sequence of messages.
    async fn compact(
        &self,
        messages: &[crate::soul::message::Message],
        llm: &crate::llm::Llm,
        custom_instruction: &str,
    ) -> crate::error::Result<CompactionResult>;
}

/// Simple compaction strategy that preserves the last N messages.
#[derive(Debug)]
pub struct SimpleCompaction {
    max_preserved_messages: usize,
}

impl SimpleCompaction {
    pub fn new(max_preserved_messages: usize) -> Self {
        Self {
            max_preserved_messages,
        }
    }

    /// Prepares the compaction by splitting messages into compactable and preserved sets.
    pub fn prepare(
        &self,
        messages: &[crate::soul::message::Message],
        custom_instruction: &str,
    ) -> (Option<crate::soul::message::Message>, Vec<crate::soul::message::Message>) {
        if messages.is_empty() || self.max_preserved_messages == 0 {
            return (None, messages.to_vec());
        }

        let history: Vec<_> = messages.to_vec();
        let mut preserve_start_index = history.len();
        let mut n_preserved = 0;

        for (index, msg) in history.iter().enumerate().rev() {
            if msg.role == "user" || msg.role == "assistant" {
                n_preserved += 1;
                if n_preserved == self.max_preserved_messages {
                    preserve_start_index = index;
                    break;
                }
            }
        }

        if n_preserved < self.max_preserved_messages {
            return (None, history);
        }

        let to_compact = &history[..preserve_start_index];
        let to_preserve = history[preserve_start_index..].to_vec();

        if to_compact.is_empty() {
            return (None, to_preserve);
        }

        let mut compact_message = crate::soul::message::Message {
            role: "user".into(),
            content: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        };

        for (i, msg) in to_compact.iter().enumerate() {
            compact_message.content.push(crate::soul::message::ContentPart::Text {
                text: format!("## Message {}\nRole: {}\nContent:\n", i + 1, msg.role),
            });
            for part in &msg.content {
                if let crate::soul::message::ContentPart::Text { text } = part {
                    compact_message.content.push(crate::soul::message::ContentPart::Text {
                        text: text.clone(),
                    });
                }
            }
        }

        let mut prompt_text = "\n".to_string();
        prompt_text.push_str("Compact the above conversation context into a concise summary.");
        if !custom_instruction.is_empty() {
            prompt_text.push_str(&format!(
                "\n\n**User's Custom Compaction Instruction:**\n{custom_instruction}"
            ));
        }
        compact_message.content.push(crate::soul::message::ContentPart::Text {
            text: prompt_text,
        });

        (Some(compact_message), to_preserve)
    }
}

impl Default for SimpleCompaction {
    fn default() -> Self {
        Self::new(2)
    }
}

#[async_trait::async_trait]
impl Compaction for SimpleCompaction {
    #[tracing::instrument(level = "debug")]
    async fn compact(
        &self,
        messages: &[crate::soul::message::Message],
        llm: &crate::llm::Llm,
        custom_instruction: &str,
    ) -> crate::error::Result<CompactionResult> {
        let (compact_message, to_preserve) = self.prepare(messages, custom_instruction);
        if compact_message.is_none() {
            return Ok(CompactionResult {
                messages: to_preserve,
                usage: None,
            });
        }

        tracing::debug!("Compacting context...");
        let compact_msg = compact_message.unwrap();
        let history = vec![compact_msg];
        match llm.chat(None, &history, None).await {
            Ok(summary) => {
                let compacted_messages = vec![
                    crate::soul::message::Message {
                        role: "user".into(),
                        content: vec![crate::soul::message::ContentPart::Text {
                            text: "Previous context has been compacted. Here is the summary:".into(),
                        }],
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    crate::soul::message::Message {
                        role: "assistant".into(),
                        content: summary.content,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                ];
                let mut result = compacted_messages;
                result.extend(to_preserve);
                Ok(CompactionResult {
                    messages: result,
                    usage: None,
                })
            }
            Err(e) => {
                tracing::warn!("LLM compaction failed: {}", e);
                // Fall back to keeping preserved messages only.
                Ok(CompactionResult {
                    messages: to_preserve,
                    usage: None,
                })
            }
        }
    }
}
