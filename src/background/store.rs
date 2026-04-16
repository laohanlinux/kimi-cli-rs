use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persisted metadata for a background task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub command: String,
    pub created_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

/// Persistent store for background task metadata.
#[derive(Debug, Clone)]
pub struct BackgroundTaskStore {
    root: PathBuf,
}

impl BackgroundTaskStore {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    fn store_path(&self) -> PathBuf {
        self.root.join("background_tasks.json")
    }

    /// Loads all persisted task records.
    pub fn load_records(&self) -> HashMap<String, TaskRecord> {
        let path = self.store_path();
        if !path.exists() {
            return HashMap::new();
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(%e, "failed to read background task store");
                return HashMap::new();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(%e, "failed to parse background task store");
            HashMap::new()
        })
    }

    /// Saves all task records.
    pub fn save_records(&self, records: &HashMap<String, TaskRecord>) -> crate::error::Result<()> {
        let path = self.store_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(records)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Saves a single task record.
    pub fn save_task(&self, record: &TaskRecord) -> crate::error::Result<()> {
        let mut records = self.load_records();
        records.insert(record.id.clone(), record.clone());
        self.save_records(&records)
    }

    /// Removes a task record.
    pub fn remove_task(&self, id: &str) -> crate::error::Result<()> {
        let mut records = self.load_records();
        records.remove(id);
        self.save_records(&records)
    }

    /// Returns the output log path for a task.
    pub fn output_path(&self, id: &str) -> PathBuf {
        self.root.join("logs").join(format!("{id}.log"))
    }

    /// Appends output text to a task's log file.
    pub fn append_output(&self, id: &str, text: &str) -> crate::error::Result<()> {
        let path = self.output_path(id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(text.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = BackgroundTaskStore::new(tmp.path());
        let record = TaskRecord {
            id: "task-1".into(),
            command: "echo hello".into(),
            created_at: 1234567890.0,
            exit_code: Some(0),
            running: false,
            failure_reason: None,
        };
        store.save_task(&record).unwrap();
        let loaded = store.load_records();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["task-1"].command, "echo hello");
    }

    #[test]
    fn store_append_output() {
        let tmp = tempfile::tempdir().unwrap();
        let store = BackgroundTaskStore::new(tmp.path());
        store.append_output("t1", "hello\n").unwrap();
        store.append_output("t1", "world\n").unwrap();
        let path = store.output_path("t1");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "hello\nworld\n");
    }
}
