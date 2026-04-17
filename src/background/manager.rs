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

    /// Returns true if the task is still running (blocking).
    pub fn is_running_blocking(&self) -> bool {
        match self.running.try_lock() {
            Ok(guard) => *guard,
            Err(_) => false,
        }
    }

    /// Returns the exit code (blocking).
    pub fn exit_code_blocking(&self) -> Option<i32> {
        match self.exit_code.try_lock() {
            Ok(guard) => *guard,
            Err(_) => None,
        }
    }
}

/// Manages background task lifecycle.
#[derive(Debug, Clone)]
pub struct BackgroundTaskManager {
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    max_running_tasks: Arc<Mutex<usize>>,
    store: Option<Arc<crate::background::store::BackgroundTaskStore>>,
}

impl Default for BackgroundTaskManager {
    fn default() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            max_running_tasks: Arc::new(Mutex::new(10)),
            store: None,
        }
    }
}

impl BackgroundTaskManager {
    /// Binds the runtime configuration to the manager.
    pub async fn bind_runtime(&mut self, runtime: &crate::soul::agent::Runtime) {
        let max = runtime.config.background.max_running_tasks;
        *self.max_running_tasks.lock().await = max;
        let store_dir = runtime.session.dir().join("background");
        self.store = Some(Arc::new(
            crate::background::store::BackgroundTaskStore::new(&store_dir),
        ));
        tracing::debug!(max_running_tasks = max, dir = %store_dir.display(), "bound runtime to background manager");
    }

    /// Returns a copy of the manager scoped to the given role.
    pub fn copy_for_role(&self, role: &str) -> Self {
        tracing::debug!(role, "copying background task manager for role");
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            max_running_tasks: self.max_running_tasks.clone(),
            store: self.store.clone(),
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

        let child = {
            let mut cmd = Command::new(shell_path);
            crate::utils::subprocess_env::apply_to_tokio(
                &mut cmd,
                crate::utils::subprocess_env::get_clean_env(),
            );
            cmd.args(&args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        }
        .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;

        *task.child.lock().await = Some(child);

        let stdout = task.stdout.clone();
        let stderr = task.stderr.clone();
        let exit_code = task.exit_code.clone();
        let running = task.running.clone();
        let child_arc = task.child.clone();

        let store_clone = self.store.clone();
        let id_clone = id.clone();
        let command_clone = command.to_string();
        let created_at_clone = task.created_at;
        tokio::spawn(async move {
            let child = child_arc.lock().await.take();
            let Some(mut child) = child else {
                *running.lock().await = false;
                return;
            };

            let store = store_clone.clone();
            let task_id = id_clone.clone();
            let stdout_handle = async {
                if let Some(sout) = child.stdout.take() {
                    let mut reader = BufReader::new(sout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        stdout.lock().await.push_str(&line);
                        stdout.lock().await.push('\n');
                        if let Some(ref s) = store {
                            let _ = s.append_output(&task_id, &format!("{line}\n"));
                        }
                    }
                }
            };
            let stderr_handle = async {
                if let Some(serr) = child.stderr.take() {
                    let mut reader = BufReader::new(serr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        stderr.lock().await.push_str(&line);
                        stderr.lock().await.push('\n');
                        if let Some(ref s) = store {
                            let _ = s.append_output(&task_id, &format!("{line}\n"));
                        }
                    }
                }
            };
            let ((), ()) = tokio::join!(stdout_handle, stderr_handle);
            let code = match child.wait().await {
                Ok(status) => {
                    let c = status.code();
                    *exit_code.lock().await = c;
                    c
                }
                Err(e) => {
                    tracing::warn!("background task wait failed: {}", e);
                    None
                }
            };
            *running.lock().await = false;

            if let Some(ref s) = store {
                let record = crate::background::store::TaskRecord {
                    id: task_id,
                    command: command_clone,
                    created_at: created_at_clone
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64(),
                    exit_code: code,
                    running: false,
                    failure_reason: None,
                };
                let _ = s.save_task(&record);
            }
        });

        if let Some(ref s) = self.store {
            let record = crate::background::store::TaskRecord {
                id: id.clone(),
                command: command.to_string(),
                created_at: task
                    .created_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
                exit_code: None,
                running: true,
                failure_reason: None,
            };
            let _ = s.save_task(&record);
        }

        self.tasks.write().await.insert(id.clone(), task.clone());
        Ok(task)
    }

    /// Lists all known tasks, optionally filtering to running ones.
    pub async fn list(&self, active_only: bool) -> Vec<BackgroundTask> {
        let tasks: Vec<_> = self.tasks.read().await.values().cloned().collect();
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in tasks {
            seen.insert(t.id.clone());
            if !active_only || t.is_running().await {
                result.push(t);
            }
        }
        if !active_only {
            if let Some(ref s) = self.store {
                for (id, record) in s.load_records() {
                    if seen.contains(&id) {
                        continue;
                    }
                    let task = BackgroundTask {
                        id: record.id,
                        command: record.command,
                        created_at: std::time::UNIX_EPOCH
                            + std::time::Duration::from_secs_f64(record.created_at),
                        stdout: Arc::new(Mutex::new(String::new())),
                        stderr: Arc::new(Mutex::new(String::new())),
                        exit_code: Arc::new(Mutex::new(record.exit_code)),
                        running: Arc::new(Mutex::new(record.running)),
                        failure_reason: Arc::new(Mutex::new(record.failure_reason)),
                        child: Arc::new(Mutex::new(None)),
                    };
                    result.push(task);
                }
            }
        }
        result
    }

    /// Looks up a task by ID.
    pub async fn get(&self, id: &str) -> Option<BackgroundTask> {
        if let Some(task) = self.tasks.read().await.get(id).cloned() {
            return Some(task);
        }
        if let Some(ref s) = self.store {
            if let Some(record) = s.load_records().get(id).cloned() {
                return Some(BackgroundTask {
                    id: record.id,
                    command: record.command,
                    created_at: std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs_f64(record.created_at),
                    stdout: Arc::new(Mutex::new(String::new())),
                    stderr: Arc::new(Mutex::new(String::new())),
                    exit_code: Arc::new(Mutex::new(record.exit_code)),
                    running: Arc::new(Mutex::new(record.running)),
                    failure_reason: Arc::new(Mutex::new(record.failure_reason)),
                    child: Arc::new(Mutex::new(None)),
                });
            }
        }
        None
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
        if let Some(ref s) = self.store {
            return s.output_path(id);
        }
        crate::share::get_share_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("logs")
            .join(format!("{id}.log"))
    }

    /// Whether the on-disk log for this task exists so agents can use `ReadFile` on
    /// [`Self::resolve_output_path`] to page through full output (matches Python `full_output_available`).
    pub fn output_log_file_available(&self, id: &str) -> bool {
        self.resolve_output_path(id).is_file()
    }

    /// Reads a chunk of task output from memory or store.
    pub async fn read_output(&self, id: &str, offset: u64, max_bytes: usize) -> OutputChunk {
        let text = if let Some(task) = self.get(id).await {
            task.output().await
        } else if let Some(ref s) = self.store {
            std::fs::read_to_string(s.output_path(id)).unwrap_or_default()
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

    /// Reconciles running tasks against a notification claim deadline.
    pub async fn reconcile(&self, before_claim_ms: u64) -> Vec<BackgroundTask> {
        let all = self.list(false).await;
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_millis(before_claim_ms);
        let mut result = Vec::new();
        for t in all {
            if t.is_running().await && t.created_at < cutoff {
                result.push(t);
            }
        }
        result
    }

    /// Formats a single task for display.
    pub fn format_task(task: &BackgroundTask) -> String {
        crate::background::summary::summarize(task)
    }

    /// Creates a background agent task that runs a subagent.
    #[tracing::instrument(level = "debug", skip(self, runtime))]
    pub async fn create_agent_task(
        &self,
        req: crate::subagents::runner::ForegroundRunRequest,
        runtime: crate::soul::agent::Runtime,
        timeout_s: Option<u64>,
    ) -> crate::error::Result<BackgroundTask> {
        let id = format!("agent-{}-{}", req.requested_type, uuid::Uuid::new_v4());
        let task = BackgroundTask::new(id.clone(), format!("agent: {}", req.description));

        let runner = crate::subagents::runner::ForegroundSubagentRunner::new(runtime);
        let task_clone = task.clone();
        let store_clone = self.store.clone();
        let id_clone = id.clone();
        let command_clone = task.command.clone();
        let created_at_clone = task.created_at;
        let timeout_s = timeout_s.map(|t| t.min(3600));

        tokio::spawn(async move {
            let result = if let Some(t) = timeout_s {
                match tokio::time::timeout(tokio::time::Duration::from_secs(t), runner.run(&req))
                    .await
                {
                    Ok(r) => r,
                    Err(_) => {
                        *task_clone.failure_reason.lock().await =
                            Some(format!("Timed out after {}s", t));
                        crate::soul::message::ToolReturnValue::Error {
                            error: format!("Agent timed out after {t}s."),
                        }
                    }
                }
            } else {
                runner.run(&req).await
            };

            match result {
                crate::soul::message::ToolReturnValue::Ok { output, .. } => {
                    *task_clone.stdout.lock().await = output;
                    *task_clone.exit_code.lock().await = Some(0);
                }
                crate::soul::message::ToolReturnValue::Parts { parts } => {
                    let text = parts
                        .iter()
                        .filter_map(|p| match p {
                            crate::soul::message::ContentPart::Text { text } => Some(text.as_str()),
                            crate::soul::message::ContentPart::Think { thought } => {
                                Some(thought.as_str())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    *task_clone.stdout.lock().await = text;
                    *task_clone.exit_code.lock().await = Some(0);
                }
                crate::soul::message::ToolReturnValue::Error { error } => {
                    *task_clone.stderr.lock().await = error.clone();
                    *task_clone.exit_code.lock().await = Some(1);
                    if task_clone.failure_reason.lock().await.is_none() {
                        *task_clone.failure_reason.lock().await = Some(error);
                    }
                }
            }
            *task_clone.running.lock().await = false;

            if let Some(ref s) = store_clone {
                let record = crate::background::store::TaskRecord {
                    id: id_clone,
                    command: command_clone,
                    created_at: created_at_clone
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64(),
                    exit_code: *task_clone.exit_code.lock().await,
                    running: false,
                    failure_reason: task_clone.failure_reason.lock().await.clone(),
                };
                let _ = s.save_task(&record);
            }
        });

        self.tasks.write().await.insert(id.clone(), task.clone());
        if let Some(ref s) = self.store {
            let record = crate::background::store::TaskRecord {
                id: id.clone(),
                command: task.command.clone(),
                created_at: task
                    .created_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
                exit_code: None,
                running: true,
                failure_reason: None,
            };
            let _ = s.save_task(&record);
        }

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

            if let Some(ref s) = self.store {
                let record = crate::background::store::TaskRecord {
                    id: id.into(),
                    command: task.command.clone(),
                    created_at: task
                        .created_at
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64(),
                    exit_code: *task.exit_code.lock().await,
                    running: false,
                    failure_reason: Some(reason.into()),
                };
                let _ = s.save_task(&record);
            }
        }
        tasks.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mgr_with_store(
        store: Arc<crate::background::store::BackgroundTaskStore>,
    ) -> BackgroundTaskManager {
        BackgroundTaskManager {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            max_running_tasks: Arc::new(Mutex::new(10)),
            store: Some(store),
        }
    }

    #[test]
    fn output_log_file_available_tracks_file() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::background::store::BackgroundTaskStore::new(
            tmp.path(),
        ));
        let mgr = mgr_with_store(store.clone());
        assert!(!mgr.output_log_file_available("t1"));
        store.append_output("t1", "line\n").unwrap();
        assert!(mgr.output_log_file_available("t1"));
    }

    #[tokio::test]
    async fn background_manager_stop_kills_child() {
        let manager = BackgroundTaskManager::default();
        *manager.max_running_tasks.lock().await = 5;

        let task = manager
            .spawn("sleep 10", "/bin/sh", false)
            .await
            .expect("spawn should succeed");

        assert!(
            task.is_running().await,
            "task should be running after spawn"
        );

        let stopped = manager.stop(&task.id).await;
        assert!(stopped.is_some(), "stop should return the task");

        // Give the OS a moment to terminate the process.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        assert!(
            !task.is_running().await,
            "task should no longer be running after stop"
        );

        // Ensure the task is removed from the manager.
        assert!(
            manager.get(&task.id).await.is_none(),
            "task should be removed from manager"
        );
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
        assert!(
            result.is_err(),
            "second spawn should exceed max running tasks"
        );
    }
}
