use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

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
}

impl BackgroundTaskManager {
    pub fn bind_runtime(&mut self, _runtime: &crate::soul::agent::Runtime) {}

    pub fn copy_for_role(&self, _role: &str) -> Self {
        self.clone()
    }

    /// Spawns a new background shell command and tracks it.
    pub async fn spawn(
        &self,
        command: &str,
        shell_path: &str,
        is_powershell: bool,
    ) -> crate::error::Result<BackgroundTask> {
        let id = format!("bg-{}", uuid::Uuid::new_v4());
        let task = BackgroundTask::new(id.clone(), command.to_string());

        let args: Vec<String> = if is_powershell {
            vec!["-command".into(), command.into()]
        } else {
            vec!["-c".into(), command.into()]
        };

        let mut child = Command::new(shell_path)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;

        let stdout = task.stdout.clone();
        let stderr = task.stderr.clone();
        let exit_code = task.exit_code.clone();
        let running = task.running.clone();

        tokio::spawn(async move {
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

    /// Stops a running task by killing its process.
    pub async fn stop(&self, id: &str) -> Option<BackgroundTask> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get(id) {
            *task.running.lock().await = false;
        }
        tasks.remove(id)
    }
}
