use std::path::Path;

/// Manages user notifications.
#[derive(Debug, Clone)]
pub struct NotificationManager;

impl NotificationManager {
    pub fn new(_root: &Path, _config: crate::config::NotificationConfig) -> Self {
        Self
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self
    }
}
