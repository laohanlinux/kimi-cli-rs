use std::path::{Path, PathBuf};

/// Persistent store for notifications.
#[derive(Debug, Clone)]
pub struct NotificationStore {
    dir: PathBuf,
}

impl NotificationStore {
    /// Creates a new notification store rooted at the given directory.
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    /// Saves a notification record to disk.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn save(&self, notification: &crate::notifications::manager::Notification) -> crate::error::Result<()> {
        let _ = std::fs::create_dir_all(&self.dir);
        let path = self.dir.join(format!("{}.json", uuid::Uuid::new_v4()));
        let text = serde_json::to_string_pretty(notification)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Loads all saved notification records.
    pub fn load_all(&self) -> Vec<crate::notifications::manager::Notification> {
        let mut results = Vec::new();
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(_) => return results,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    if let Ok(notification) = serde_json::from_str(&text) {
                        results.push(notification);
                    }
                }
                Err(e) => tracing::warn!("Failed to read notification {}: {}", path.display(), e),
            }
        }
        results.sort_by(|a, b| a.title.cmp(&b.title));
        results
    }
}
