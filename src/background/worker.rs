use std::sync::Arc;
use tokio::sync::Mutex;

/// Worker that monitors background task health.
#[derive(Debug, Clone)]
pub struct BackgroundWorker {
    tasks: Arc<Mutex<Vec<crate::background::manager::BackgroundTask>>>,
    stale_after: std::time::Duration,
}

impl BackgroundWorker {
    /// Creates a new background worker.
    pub fn new(stale_after_secs: u64) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            stale_after: std::time::Duration::from_secs(stale_after_secs),
        }
    }

    /// Registers a task for monitoring.
    pub async fn monitor(&self, task: crate::background::manager::BackgroundTask) {
        self.tasks.lock().await.push(task);
    }

    /// Returns tasks that have been stale for longer than the threshold.
    pub async fn check_stale(&self) -> Vec<String> {
        let now = std::time::SystemTime::now();
        let tasks = self.tasks.lock().await;
        tasks
            .iter()
            .filter(|t| {
                let running = futures::executor::block_on(t.is_running());
                let elapsed = now.duration_since(t.created_at).unwrap_or_default();
                running && elapsed > self.stale_after
            })
            .map(|t| t.id.clone())
            .collect()
    }
}
