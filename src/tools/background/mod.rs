use async_trait::async_trait;
use serde_json::Value;

fn _ensure_root(runtime: &crate::soul::agent::Runtime) -> Option<crate::soul::message::ToolReturnValue> {
    if runtime.role != "root" {
        return Some(crate::soul::message::ToolReturnValue::Error {
            error: "Background tasks can only be managed by the root agent.".into(),
        });
    }
    None
}

const TASK_OUTPUT_PREVIEW_BYTES: usize = 32 * 1024;
const TASK_OUTPUT_READ_HINT_LINES: usize = 300;

fn _format_task_output(
    task_id: &str,
    command: &str,
    running: bool,
    exit_code: Option<i32>,
    failure_reason: Option<&str>,
    retrieval_status: &str,
    output: &str,
    output_path: &std::path::Path,
    full_output_available: bool,
    output_size_bytes: usize,
    output_preview_bytes: usize,
    output_truncated: bool,
) -> String {
    let terminal_reason = if running { "running" } else if exit_code == Some(0) { "completed" } else { "failed" };
    let output_path_str = output_path.to_string_lossy();
    let mut lines = vec![
        format!("retrieval_status: {retrieval_status}"),
        format!("task_id: {task_id}"),
        format!("status: {}", if running { "running" } else { "stopped" }),
        format!("command: {command}"),
        format!("terminal_reason: {terminal_reason}"),
    ];
    if let Some(code) = exit_code {
        lines.push(format!("exit_code: {code}"));
    }
    if let Some(reason) = failure_reason {
        lines.push(format!("reason: {reason}"));
    }
    let full_output_hint = if full_output_available {
        format!(
            "full_output_hint: Use ReadFile(path=\"{}\", line_offset=1, n_lines={}) to inspect the full log. Increase line_offset to continue paging through the file.",
            output_path_str, TASK_OUTPUT_READ_HINT_LINES
        )
    } else {
        "full_output_hint: No output file is currently available for this task.".into()
    };
    lines.push(String::new());
    lines.push(format!("output_path: {output_path_str}"));
    lines.push(format!("output_size_bytes: {output_size_bytes}"));
    lines.push(format!("output_preview_bytes: {output_preview_bytes}"));
    lines.push(format!("output_truncated: {output_truncated}"));
    lines.push(String::new());
    lines.push(format!("full_output_available: {full_output_available}"));
    lines.push("full_output_tool: ReadFile".into());
    lines.push(full_output_hint);
    let rendered_output = if output.is_empty() { "[no output available]".into() } else { output.into() };
    let final_output = if output_truncated {
        format!("[Truncated. Full output: {output_path_str}]\n\n{rendered_output}")
    } else {
        rendered_output
    };
    lines.push(String::new());
    lines.push("[output]".into());
    lines.push(final_output);
    lines.join("\n")
}

fn _format_task(task_id: &str, command: &str, running: bool, exit_code: Option<i32>) -> String {
    let status = if running { "running" } else if exit_code == Some(0) { "completed" } else { "failed" };
    format!("task_id: {task_id}\nstatus: {status}\ncommand: {command}")
}

fn _format_task_list(tasks: &[crate::background::manager::BackgroundTask], _active_only: bool) -> String {
    if tasks.is_empty() {
        return "No background tasks.".into();
    }
    let mut lines = vec![format!("Background tasks ({}):", tasks.len())];
    for t in tasks {
        let running = futures::executor::block_on(t.is_running());
        let exit_code = *futures::executor::block_on(t.exit_code.lock());
        let status = if running { "running".into() } else if let Some(c) = exit_code { format!("exited {c}") } else { "unknown".into() };
        lines.push(format!("  {} [{}] {}", t.id, status, t.command));
    }
    lines.join("\n")
}

/// Lists background tasks.
#[derive(Debug, Clone, Default)]
pub struct TaskList;

#[async_trait]
impl crate::soul::toolset::Tool for TaskList {
    fn name(&self) -> &str {
        "TaskList"
    }

    fn description(&self) -> &str {
        "List active or all background tasks."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "active_only": { "type": "boolean", "default": true },
                "limit": { "type": "integer", "default": 20, "minimum": 1, "maximum": 100 }
            }
        })
    }

    async fn call(
        &self,
        arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if let Some(err) = _ensure_root(runtime) {
            return err;
        }
        let active_only = arguments.get("active_only").and_then(|v| v.as_bool()).unwrap_or(true);
        let limit = arguments.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let mut tasks = runtime.background_tasks.list(active_only).await;
        tasks.truncate(limit);
        let output = _format_task_list(&tasks, active_only);
        crate::soul::message::ToolReturnValue::Ok {
            output,
            message: Some("Task list retrieved.".into()),
        }
    }
}

/// Gets output from a background task.
#[derive(Debug, Clone, Default)]
pub struct TaskOutput;

#[async_trait]
impl crate::soul::toolset::Tool for TaskOutput {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        "Get the current output of a background task."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "The background task ID to inspect." },
                "block": { "type": "boolean", "default": false, "description": "Whether to wait for the task to finish before returning." },
                "timeout": { "type": "integer", "default": 30, "minimum": 0, "maximum": 3600, "description": "Maximum number of seconds to wait when block=true." }
            },
            "required": ["task_id"]
        })
    }

    async fn call(
        &self,
        arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if let Some(err) = _ensure_root(runtime) {
            return err;
        }
        let task_id = arguments.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        if task_id.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "task_id is required".into(),
            };
        }
        let block = arguments.get("block").and_then(|v| v.as_bool()).unwrap_or(false);
        let timeout = arguments.get("timeout").and_then(|v| v.as_u64()).unwrap_or(30) as u64;

        let mut task = match runtime.background_tasks.get_task(task_id).await {
            Some(t) => t,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Task not found: {task_id}"),
                };
            }
        };

        if block {
            task = match runtime.background_tasks.wait(task_id, timeout).await {
                Some(t) => t,
                None => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Task not found: {task_id}"),
                    };
                }
            };
        }

        let running = task.is_running().await;
        let terminal = !running;
        let retrieval_status = if block {
            if terminal { "success" } else { "timeout" }
        } else {
            if terminal { "success" } else { "not_ready" }
        };

        let output_path = runtime.background_tasks.resolve_output_path(task_id);
        let output_size = task.output().await.len();
        let preview_offset = output_size.saturating_sub(TASK_OUTPUT_PREVIEW_BYTES) as u64;
        let chunk = runtime.background_tasks.read_output(task_id, preview_offset, TASK_OUTPUT_PREVIEW_BYTES).await;
        let output_truncated = preview_offset > 0;
        let output_preview_bytes = chunk.next_offset - chunk.offset;

        let running = task.is_running().await;
        let exit_code = *task.exit_code.lock().await;
        let output = _format_task_output(
            &task.id,
            &task.command,
            running,
            exit_code,
            task.failure_reason.lock().await.as_deref(),
            retrieval_status,
            &chunk.text.trim_end_matches('\n').to_string(),
            &output_path,
            true, // full_output_available stub
            output_size,
            output_preview_bytes as usize,
            output_truncated,
        );

        let msg = if !block && retrieval_status == "not_ready" {
            "Task snapshot retrieved."
        } else {
            "Task output retrieved."
        };

        crate::soul::message::ToolReturnValue::Ok {
            output,
            message: Some(msg.into()),
        }
    }
}

/// Stops a background task.
#[derive(Debug, Clone, Default)]
pub struct TaskStop;

#[async_trait]
impl crate::soul::toolset::Tool for TaskStop {
    fn name(&self) -> &str {
        "TaskStop"
    }

    fn description(&self) -> &str {
        "Stop a running background task."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "The background task ID to stop." },
                "reason": { "type": "string", "default": "Stopped by TaskStop", "description": "Short reason recorded when the task is stopped." }
            },
            "required": ["task_id"]
        })
    }

    async fn call(
        &self,
        arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if let Some(err) = _ensure_root(runtime) {
            return err;
        }

        let session_dir = runtime.session.dir();
        let state = crate::session_state::load_session_state(&session_dir);
        if state.plan_mode {
            return crate::soul::message::ToolReturnValue::Error {
                error: "TaskStop is not available in plan mode.".into(),
            };
        }

        let task_id = arguments.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        if task_id.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "task_id is required".into(),
            };
        }

        let view = match runtime.background_tasks.get_task(task_id).await {
            Some(t) => t,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Task not found: {task_id}"),
                };
            }
        };

        let desc = format!("Stop background task `{task_id}`");
        let result = match runtime.approval.request(
            "TaskStop",
            "stop background task",
            &desc,
            None,
        ).await {
            Ok(r) => r,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Approval request failed: {e}"),
                };
            }
        };

        if !result.approved {
            return crate::soul::message::ToolReturnValue::Error {
                error: result.rejection_error().to_string(),
            };
        }

        let reason = arguments.get("reason").and_then(|v| v.as_str()).unwrap_or("Stopped by TaskStop").trim();
        let reason = if reason.is_empty() { "Stopped by TaskStop" } else { reason };
        let task = runtime.background_tasks.kill(task_id, reason).await.unwrap_or(view);
        let running = task.is_running().await;
        let exit_code = *task.exit_code.lock().await;

        crate::soul::message::ToolReturnValue::Ok {
            output: _format_task(&task.id, &task.command, running, exit_code),
            message: Some("Task stop requested.".into()),
        }
    }
}
