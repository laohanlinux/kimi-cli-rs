/// A dynamic prompt content to be injected before an LLM step.
#[derive(Debug, Clone)]
pub struct DynamicInjection {
    pub r#type: String,
    pub content: String,
}

/// Base trait for dynamic injection providers.
#[async_trait::async_trait]
pub trait DynamicInjectionProvider: Send + Sync {
    /// Returns injections to be merged into the next LLM request.
    async fn get_injections(
        &self,
        history: &[crate::soul::message::Message],
        soul: &crate::soul::kimisoul::KimiSoul,
    ) -> Vec<DynamicInjection>;
}

/// Merges adjacent user messages to produce a clean API input sequence.
pub fn normalize_history(
    history: &[crate::soul::message::Message],
    is_notification: impl Fn(&crate::soul::message::Message) -> bool,
) -> Vec<crate::soul::message::Message> {
    if history.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<crate::soul::message::Message> = Vec::new();
    for msg in history {
        if let Some(last) = result.last_mut() {
            if last.role == msg.role
                && msg.role == "user"
                && !is_notification(last)
                && !is_notification(msg)
            {
                last.content.extend(msg.content.clone());
                continue;
            }
        }
        result.push(msg.clone());
    }
    result
}
