use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

/// A chunk of task output.
#[derive(Debug, Clone)]
pub struct OutputChunk {
    pub text: String,
    pub offset: u64,
    pub next_offset: u64,
}

/// State of a background task.
#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub created_at: std::time::SystemTime,
    pub stdout: Arc<Mutex<String>>,
    pub stderr: Arc<Mutex<String>>,
    pub exit_code: Arc<Mutex<Option<i32>>>,
    pub running: Arc<Mutex<bool>>,
    pub failure_reason: Arc<Mutex<Option<String>>>,
    child: Arc<Mutex<Option<tokio::process::Child>>>,
}

impl BackgroundTask {
    fn new(id: String, command: String) -> Self {
        Self {
            id,
            command,
            created_at: std::time::SystemTime::now(),
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            exit_code: Arc::new(Mutex::new(None)),
            running: Arc::new(Mutex::new(true)),
            failure_reason: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the concatenated stdout + stderr output.
    pub async fn output(&self) -> String {
        let out = self.stdout.lock().await.clone();
        let err = self.stderr.lock().await.clone();
        if err.is_empty() {
            out
        } else {
            format!("{out}\n{err}")
        }
    }

    /// Returns true if the task is still running.
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }
}

/// Manages background task lifecycle.
#[derive(Debug, Clone, Default)]
pub struct BackgroundTaskManager {
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    max_running_tasks: Arc<Mutex<usize>>,
}

impl BackgroundTaskManager {
    /// Binds the runtime configuration to the manager.
    pub fn bind_runtime(&mut self, runtime: &crate::soul::agent::Runtime) {
        let max = runtime.config.background.max_running_tasks;
        *self.max_running_tasks.blocking_lock() = max;
        tracing::debug!(max_running_tasks = max, "bound runtime to background manager");
    }

    /// Returns a copy of the manager scoped to the given role.
    pub fn copy_for_role(&self, role: &str) -> Self {
        tracing::debug!(role, "copying background task manager for role");
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            max_running_tasks: self.max_running_tasks.clone(),
        }
    }

    /// Spawns a new background shell command and tracks it.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn spawn(
        &self,
        command: &str,
        shell_path: &str,
        is_powershell: bool,
    ) -> crate::error::Result<BackgroundTask> {
        let active = self.list(true).await;
        let max = *self.max_running_tasks.lock().await;
        if active.len() >= max {
            return Err(crate::error::KimiCliError::Generic(format!(
                "Maximum number of running background tasks ({max}) reached"
            )));
        }

        let id = format!("bg-{}", uuid::Uuid::new_v4());
        let task = BackgroundTask::new(id.clone(), command.to_string());

        let args: Vec<String> = if is_powershell {
            vec!["-command".into(), command.into()]
        } else {
            vec!["-c".into(), command.into()]
        };

        let child = Command::new(shell_path)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;

        *task.child.lock().await = Some(child);

        let stdout = task.stdout.clone();
        let stderr = task.stderr.clone();
        let exit_code = task.exit_code.clone();
        let running = task.running.clone();
        let child_arc = task.child.clone();

        tokio::spawn(async move {
            let child = child_arc.lock().await.take();
            let Some(mut child) = child else {
                *running.lock().await = false;
                return;
            };

            let stdout_handle = async {
                if let Some(sout) = child.stdout.take() {
                    let mut reader = BufReader::new(sout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        stdout.lock().await.push_str(&line);
                        stdout.lock().await.push('\n');
                    }
                }
            };
            let stderr_handle = async {
                if let Some(serr) = child.stderr.take() {
                    let mut reader = BufReader::new(serr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        stderr.lock().await.push_str(&line);
                        stderr.lock().await.push('\n');
                    }
                }
            };
            let ((), ()) = tokio::join!(stdout_handle, stderr_handle);
            match child.wait().await {
                Ok(status) => {
                    *exit_code.lock().await = status.code();
                }
                Err(e) => {
                    tracing::warn!("background task wait failed: {}", e);
                }
            }
            *running.lock().await = false;
        });

        self.tasks.write().await.insert(id.clone(), task.clone());
        Ok(task)
    }

    /// Lists all known tasks, optionally filtering to running ones.
    pub async fn list(&self, active_only: bool) -> Vec<BackgroundTask> {
        let tasks: Vec<_> = self.tasks.read().await.values().cloned().collect();
        let mut result = Vec::new();
        for t in tasks {
            if !active_only || t.is_running().await {
                result.push(t);
            }
        }
        result
    }

    /// Looks up a task by ID.
    pub async fn get(&self, id: &str) -> Option<BackgroundTask> {
        self.tasks.read().await.get(id).cloned()
    }

    /// Alias for get.
    pub async fn get_task(&self, id: &str) -> Option<BackgroundTask> {
        self.get(id).await
    }

    /// Waits for a task to reach a terminal state.
    pub async fn wait(&self, id: &str, timeout_s: u64) -> Option<BackgroundTask> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_s);
        loop {
            if let Some(task) = self.get(id).await {
                if !task.is_running().await {
                    return Some(task);
                }
            } else {
                return None;
            }
            if tokio::time::Instant::now() >= deadline {
                return self.get(id).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }

    /// Resolves the output log file path for a task.
    pub fn resolve_output_path(&self, id: &str) -> std::path::PathBuf {
        crate::share::get_share_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("logs")
            .join(format!("{id}.log"))
    }

    /// Reads a chunk of task output from memory.
    pub async fn read_output(&self, id: &str, offset: u64, max_bytes: usize) -> OutputChunk {
        let text = if let Some(task) = self.get(id).await {
            task.output().await
        } else {
            String::new()
        };
        let bytes = text.as_bytes();
        let start = (offset as usize).min(bytes.len());
        let end = (start + max_bytes).min(bytes.len());
        let chunk_text = String::from_utf8_lossy(&bytes[start..end]).to_string();
        OutputChunk {
            text: chunk_text,
            offset,
            next_offset: end as u64,
        }
    }

    /// Creates a background agent task (stub).
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn create_agent_task(
        &self,
        agent_id: &str,
        subagent_type: &str,
        prompt: &str,
        description: &str,
        _tool_call_id: &str,
        _model_override: Option<&str>,
        timeout_s: Option<u64>,
        _resumed: bool,
    ) -> crate::error::Result<BackgroundTask> {
        tracing::info!(
            agent_id = %agent_id,
            subagent_type = %subagent_type,
            "background agent task creation is a stub in the Rust port"
        );
        let id = format!("agent-{}-{}", subagent_type, uuid::Uuid::new_v4());
        let task = BackgroundTask::new(
            id,
            format!("agent {}: {} (prompt: {})", agent_id, description, prompt),
        );
        if let Some(t) = timeout_s {
            tokio::spawn({
                let task = task.clone();
                async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(t)).await;
                    let _ = task.child.lock().await.take();
                    *task.running.lock().await = false;
                    *task.exit_code.lock().await = Some(0);
                }
            });
        }
        self.tasks.write().await.insert(task.id.clone(), task.clone());
        Ok(task)
    }

    /// Stops a running task by killing its process.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn stop(&self, id: &str) -> Option<BackgroundTask> {
        self.kill(id, "Stopped by TaskStop").await
    }

    /// Kills a task with a recorded reason.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn kill(&self, id: &str, reason: &str) -> Option<BackgroundTask> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get(id) {
            if let Some(mut child) = task.child.lock().await.take() {
                if let Err(e) = child.kill().await {
                    tracing::warn!(task_id = %id, "failed to kill background task: {}", e);
                }
            }
            *task.running.lock().await = false;
            *task.failure_reason.lock().await = Some(reason.to_string());
        }
        tasks.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn background_manager_stop_kills_child() {
        let manager = BackgroundTaskManager::default();
        *manager.max_running_tasks.lock().await = 5;

        let task = manager
            .spawn("sleep 10", "/bin/sh", false)
            .await
            .expect("spawn should succeed");

        assert!(task.is_running().await, "task should be running after spawn");

        let stopped = manager.stop(&task.id).await;
        assert!(stopped.is_some(), "stop should return the task");

        // Give the OS a moment to terminate the process.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        assert!(
            !task.is_running().await,
            "task should no longer be running after stop"
        );

        // Ensure the task is removed from the manager.
        assert!(manager.get(&task.id).await.is_none(), "task should be removed from manager");
    }

    #[tokio::test]
    async fn background_manager_enforces_max_tasks() {
        let manager = BackgroundTaskManager::default();
        *manager.max_running_tasks.lock().await = 1;

        let _task1 = manager
            .spawn("sleep 10", "/bin/sh", false)
            .await
            .expect("first spawn should succeed");

        let result = manager.spawn("sleep 10", "/bin/sh", false).await;
        assert!(result.is_err(), "second spawn should exceed max running tasks");
    }
}
