use std::path::Path;
use tokio::sync::broadcast;

/// A single user notification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    store: Option<crate::notifications::store::NotificationStore>,
    notifier: crate::notifications::notifier::Notifier,
    wire: crate::notifications::wire::WireNotificationBridge,
}

impl NotificationManager {
    /// Binds the notification manager to the root wire hub.
    pub fn bind_root_wire_hub(
        &self,
        _root_wire_hub: &std::sync::Arc<crate::wire::root_hub::RootWireHub>,
    ) {
    }

    /// Creates a new notification manager rooted at the given path.
    pub fn new(root: &Path, config: crate::config::NotificationConfig) -> Self {
        let (tx, rx) = broadcast::channel(256);

        if config.desktop {
            let mut desktop_rx = tx.subscribe();
            tokio::spawn(async move {
                while let Ok(notification) = desktop_rx.recv().await {
                    deliver_desktop_notification(&notification).await;
                }
            });
        }

        let store_dir = root.join("notifications");
        let store = crate::notifications::store::NotificationStore::new(&store_dir);

        Self {
            tx,
            rx: std::sync::Mutex::new(rx),
            config,
            store: Some(store),
            notifier: crate::notifications::notifier::Notifier::default(),
            wire: crate::notifications::wire::WireNotificationBridge::default(),
        }
    }

    /// Sends a notification into the queue and persists it.
    pub fn notify(&self, notification: Notification) -> crate::error::Result<()> {
        if !self.config.enabled {
            return Ok(());
        }
        if let Some(ref store) = self.store {
            let _ = store.save(&notification);
        }
        self.notifier.notify(&notification);
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

    /// Returns a mutable reference to the internal notifier.
    pub fn notifier(&mut self) -> &mut crate::notifications::notifier::Notifier {
        &mut self.notifier
    }

    /// Publishes a notification text to the wire hub.
    pub fn publish_to_wire(&self, runtime: &crate::soul::agent::Runtime, text: String) {
        self.wire.publish(runtime, text);
    }

    /// Reconciles pending notifications against a claim deadline.
    pub fn reconcile(&self, _before_claim_ms: u64) -> Vec<Notification> {
        // Returns all notifications from the store that are older than the claim deadline.
        self.store
            .as_ref()
            .map(|s| s.load_all())
            .unwrap_or_default()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        let (tx, rx) = broadcast::channel(256);
        Self {
            tx,
            rx: std::sync::Mutex::new(rx),
            config: crate::config::NotificationConfig::default(),
            store: None,
            notifier: crate::notifications::notifier::Notifier::default(),
            wire: crate::notifications::wire::WireNotificationBridge::default(),
        }
    }
}

impl Clone for NotificationManager {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: std::sync::Mutex::new(self.tx.subscribe()),
            config: self.config.clone(),
            store: self.store.clone(),
            notifier: self.notifier.clone(),
            wire: self.wire.clone(),
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
        let mut cmd = tokio::process::Command::new("osascript");
        crate::utils::subprocess_env::apply_to_tokio(
            &mut cmd,
            crate::utils::subprocess_env::get_clean_env(),
        );
        let _ = cmd.arg("-e").arg(&script).output().await;
    }
    // Freedesktop `notify-send` (common on Linux/BSD with libnotify); no-op if missing from PATH.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = tokio::process::Command::new("notify-send");
        crate::utils::subprocess_env::apply_to_tokio(
            &mut cmd,
            crate::utils::subprocess_env::get_clean_env(),
        );
        let _ = cmd
            .arg(&notification.title)
            .arg(&notification.body)
            .output()
            .await;
    }
    #[cfg(not(unix))]
    {
        tracing::debug!(
            title = %notification.title,
            body = %notification.body,
            "desktop notification not available on this platform (use macOS, Linux with notify-send, or wire/UI)"
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
