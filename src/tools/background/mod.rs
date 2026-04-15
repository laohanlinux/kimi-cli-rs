use async_trait::async_trait;

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

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "ID of the background task" },
                "block": { "type": "boolean", "default": false }
            },
            "required": ["task_id"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let task_id = arguments.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        if task_id.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "task_id is required".into(),
            };
        }
        match runtime.background_tasks.get(task_id).await {
            Some(task) => {
                let output = task.output().await;
                let running = task.is_running().await;
                let exit = *task.exit_code.lock().await;
                let status = if running {
                    "running"
                } else {
                    match exit {
                        Some(0) => "completed",
                        Some(_) => "failed",
                        None => "unknown",
                    }
                };
                crate::soul::message::ToolReturnValue::Ok {
                    output: format!("Status: {status}\n\n{output}"),
                    message: None,
                }
            }
            None => crate::soul::message::ToolReturnValue::Error {
                error: format!("Task {task_id} not found"),
            },
        }
    }
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

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "active_only": { "type": "boolean", "default": true }
            }
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let active_only = arguments.get("active_only").and_then(|v| v.as_bool()).unwrap_or(true);
        let tasks = runtime.background_tasks.list(active_only).await;
        if tasks.is_empty() {
            return crate::soul::message::ToolReturnValue::Ok {
                output: if active_only {
                    "No active background tasks.".into()
                } else {
                    "No background tasks.".into()
                },
                message: None,
            };
        }
        let mut lines = vec![format!("Background tasks ({}):", tasks.len())];
        for t in tasks {
            let running = t.is_running().await;
            let exit = *t.exit_code.lock().await;
            let status = if running {
                "running".into()
            } else {
                match exit {
                    Some(code) => format!("exited {code}"),
                    None => "unknown".into(),
                }
            };
            lines.push(format!("  {} [{}] {}", t.id, status, t.command));
        }
        crate::soul::message::ToolReturnValue::Ok {
            output: lines.join("\n"),
            message: None,
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

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "ID of the background task" }
            },
            "required": ["task_id"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let task_id = arguments.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        if task_id.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "task_id is required".into(),
            };
        }
        match runtime.background_tasks.stop(task_id).await {
            Some(task) => crate::soul::message::ToolReturnValue::Ok {
                output: format!("Stopped background task {} ({})", task.id, task.command),
                message: None,
            },
            None => crate::soul::message::ToolReturnValue::Error {
                error: format!("Task {task_id} not found"),
            },
        }
    }
}
