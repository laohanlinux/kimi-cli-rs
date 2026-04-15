use std::path::Path;
use tokio::sync::mpsc;

/// A single user notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub channel: String,
}

/// Manages user notifications via an async queue.
#[derive(Debug, Clone)]
pub struct NotificationManager {
    tx: mpsc::UnboundedSender<Notification>,
}

impl NotificationManager {
    /// Creates a new notification manager rooted at the given path.
    pub fn new(_root: &Path, _config: crate::config::NotificationConfig) -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { tx }
    }

    /// Sends a notification into the queue.
    pub fn notify(&self, notification: Notification) -> crate::error::Result<()> {
        self.tx
            .send(notification)
            .map_err(|_| crate::error::KimiCliError::Generic("notification queue closed".into()))
    }

    /// Returns the sender handle for notifications.
    pub fn sender(&self) -> mpsc::UnboundedSender<Notification> {
        self.tx.clone()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { tx }
    }
}
