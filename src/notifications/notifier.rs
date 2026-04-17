use std::collections::HashMap;
use std::sync::Arc;

/// Delivery abstraction for notifications.
#[derive(Clone, Default)]
pub struct Notifier {
    handlers: HashMap<String, Arc<dyn Fn(&crate::notifications::manager::Notification) + Send + Sync>>,
}

impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifier")
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Notifier {
    /// Registers a notification delivery handler.
    pub fn register<F>(&mut self, name: &str, handler: F)
    where
        F: Fn(&crate::notifications::manager::Notification) + Send + Sync + 'static,
    {
        self.handlers.insert(name.into(), Arc::new(handler));
    }

    /// Dispatches a notification to all registered handlers.
    pub fn notify(&self, notification: &crate::notifications::manager::Notification) {
        for (name, handler) in &self.handlers {
            tracing::debug!(handler = %name, title = %notification.title, "delivering notification");
            handler(notification);
        }
    }
}
