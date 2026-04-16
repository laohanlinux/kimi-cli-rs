use std::path::Path;
use tokio::sync::broadcast;

/// A single user notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub channel: String,
}

/// Manages user notifications via an async queue.
#[derive(Debug)]
pub struct NotificationManager {
    tx: broadcast::Sender<Notification>,
    rx: std::sync::Mutex<broadcast::Receiver<Notification>>,
    config: crate::config::NotificationConfig,
}

impl NotificationManager {
    /// Binds the notification manager to the root wire hub.
    pub fn bind_root_wire_hub(&self, _root_wire_hub: &std::sync::Arc<crate::wire::root_hub::RootWireHub>) {}

    /// Creates a new notification manager rooted at the given path.
    pub fn new(_root: &Path, config: crate::config::NotificationConfig) -> Self {
        let (tx, rx) = broadcast::channel(256);

        if config.desktop {
            let mut desktop_rx = tx.subscribe();
            tokio::spawn(async move {
                while let Ok(notification) = desktop_rx.recv().await {
                    deliver_desktop_notification(&notification).await;
                }
            });
        }

        Self {
            tx,
            rx: std::sync::Mutex::new(rx),
            config,
        }
    }

    /// Sends a notification into the queue.
    pub fn notify(&self, notification: Notification) -> crate::error::Result<()> {
        if !self.config.enabled {
            return Ok(());
        }
        self.tx
            .send(notification)
            .map(|_| ())
            .map_err(|_| crate::error::KimiCliError::Generic("notification queue closed".into()))
    }

    /// Returns the sender handle for notifications.
    pub fn sender(&self) -> broadcast::Sender<Notification> {
        self.tx.clone()
    }

    /// Tries to receive pending notifications without blocking.
    pub fn try_recv(&self) -> Option<Notification> {
        if !self.config.enabled {
            return None;
        }
        self.rx.lock().ok()?.try_recv().ok()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        let (tx, rx) = broadcast::channel(256);
        Self {
            tx,
            rx: std::sync::Mutex::new(rx),
            config: crate::config::NotificationConfig::default(),
        }
    }
}

impl Clone for NotificationManager {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: std::sync::Mutex::new(self.tx.subscribe()),
            config: self.config.clone(),
        }
    }
}

/// Attempts to deliver a desktop notification.
#[tracing::instrument(level = "debug", skip(notification))]
async fn deliver_desktop_notification(notification: &Notification) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {:?} with title {:?}",
            notification.body, notification.title
        );
        let _ = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Desktop notifications are only implemented for macOS in this port.
        tracing::debug!(
            title = %notification.title,
            body = %notification.body,
            "desktop notification (stub on non-macOS)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_manager_default() {
        let manager = NotificationManager::default();
        assert!(manager.try_recv().is_none());
    }

    #[tokio::test]
    async fn notification_manager_send_and_recv() {
        let manager = NotificationManager::default();
        let notification = Notification {
            title: "Hello".into(),
            body: "World".into(),
            channel: "test".into(),
        };
        manager.notify(notification.clone()).unwrap();
        let received = manager.try_recv().unwrap();
        assert_eq!(received.title, "Hello");
        assert_eq!(received.body, "World");
    }

    #[tokio::test]
    async fn notification_manager_clone_receives() {
        let manager = NotificationManager::default();
        let cloned = manager.clone();
        let notification = Notification {
            title: "Test".into(),
            body: "Body".into(),
            channel: "test".into(),
        };
        manager.notify(notification).unwrap();
        let received = cloned.try_recv().unwrap();
        assert_eq!(received.title, "Test");
    }

    #[tokio::test]
    async fn notification_manager_disabled_drops() {
        let mut config = crate::config::NotificationConfig::default();
        config.enabled = false;
        let manager = NotificationManager::new(Path::new("."), config);
        let notification = Notification {
            title: "Hello".into(),
            body: "World".into(),
            channel: "test".into(),
        };
        manager.notify(notification).unwrap();
        assert!(manager.try_recv().is_none());
    }
}
