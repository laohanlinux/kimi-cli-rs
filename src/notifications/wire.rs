/// Wire protocol integration for notifications.
#[derive(Debug, Clone, Default)]
pub struct WireNotificationBridge;

impl WireNotificationBridge {
    /// Publishes a notification to the root wire hub if available.
    pub fn publish(&self, runtime: &crate::soul::agent::Runtime, text: String) {
        if let Some(ref hub) = runtime.root_wire_hub {
            hub.publish(crate::wire::types::WireMessage::Notification { text });
        }
    }
}
