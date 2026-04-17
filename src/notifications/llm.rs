/// LLM sink for notifications.
#[derive(Debug, Clone, Default)]
pub struct LlmNotificationSink;

impl LlmNotificationSink {
    /// Consumes a notification and optionally produces an LLM-readable summary.
    pub fn absorb(
        &self,
        notification: &crate::notifications::manager::Notification,
    ) -> Option<String> {
        if notification.channel == "system" {
            Some(format!(
                "[system] {}: {}",
                notification.title, notification.body
            ))
        } else {
            None
        }
    }
}
