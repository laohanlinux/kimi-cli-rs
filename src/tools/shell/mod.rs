use async_trait::async_trait;
use std::process::Stdio;

/// Maximum timeout for foreground shell commands in seconds.
const MAX_FOREGROUND_TIMEOUT: u64 = 5 * 60;
const MAX_BACKGROUND_TIMEOUT: u64 = 24 * 60 * 60;

/// Executes shell commands.
#[derive(Debug, Clone)]
pub struct Shell {
    pub shell_path: String,
    pub is_powershell: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            shell_path: "/bin/bash".into(),
            is_powershell: false,
        }
    }
}

#[async_trait]
impl crate::soul::toolset::Tool for Shell {
    fn name(&self) -> &str {
        "Shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to execute" },
                "timeout": { "type": "integer", "description": "Timeout in seconds", "default": 60 },
                "run_in_background": { "type": "boolean", "default": false },
                "description": { "type": "string", "default": "" }
            },
            "required": ["command"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let command = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if command.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Command cannot be empty.".into(),
            };
        }

        let timeout = arguments
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(60)
            .clamp(1, MAX_BACKGROUND_TIMEOUT);
        let run_in_background = arguments
            .get("run_in_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if run_in_background {
            match runtime
                .background_tasks
                .spawn(command, &self.shell_path, self.is_powershell)
                .await
            {
                Ok(task) => {
                    return crate::soul::message::ToolReturnValue::Ok {
                        output: format!(
                            "Background task started: {} (id: {})",
                            task.command, task.id
                        ),
                        message: Some(
                            "Use TaskOutput to check progress or TaskList to see all tasks."
                                .into(),
                        ),
                    };
                }
                Err(e) => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to spawn background task: {e}"),
                    };
                }
            }
        }

        if timeout > MAX_FOREGROUND_TIMEOUT {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!(
                    "timeout must be <= {MAX_FOREGROUND_TIMEOUT}s for foreground commands; \
                     use run_in_background=true for longer timeouts (up to {MAX_BACKGROUND_TIMEOUT}s)"
                ),
            };
        }

        let args = if self.is_powershell {
            vec!["-command", command]
        } else {
            vec!["-c", command]
        };

        let mut child = match tokio::process::Command::new(&self.shell_path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to spawn shell: {e}"),
                };
            }
        };

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let stdout_handle = tokio::spawn(async move {
            let mut buf = tokio::io::BufReader::new(stdout);
            let mut out = String::new();
            use tokio::io::AsyncBufReadExt;
            let mut lines = buf.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                out.push_str(&line);
                out.push('\n');
            }
            out
        });

        let stderr_handle = tokio::spawn(async move {
            let mut buf = tokio::io::BufReader::new(stderr);
            let mut out = String::new();
            use tokio::io::AsyncBufReadExt;
            let mut lines = buf.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                out.push_str(&line);
                out.push('\n');
            }
            out
        });

        let result = tokio::time::timeout(tokio::time::Duration::from_secs(timeout), child.wait()).await;
        let (stdout_text, stderr_text) = match (stdout_handle.await, stderr_handle.await) {
            (Ok(o), Ok(e)) => (o, e),
            _ => (String::new(), String::new()),
        };

        match result {
            Ok(Ok(status)) => {
                let mut output = String::new();
                if !stdout_text.is_empty() {
                    output.push_str(&stdout_text);
                }
                if !stderr_text.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&stderr_text);
                }
                let code = status.code().unwrap_or(-1);
                if code == 0 {
                    crate::soul::message::ToolReturnValue::Ok {
                        output: output.trim_end().to_string(),
                        message: Some("Command executed successfully.".into()),
                    }
                } else {
                    crate::soul::message::ToolReturnValue::Error {
                        error: format!("Command failed with exit code: {code}.\n{output}"),
                    }
                }
            }
            Ok(Err(e)) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Command execution failed: {e}"),
            },
            Err(_) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Command killed by timeout ({timeout}s)"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soul::toolset::Tool;

    #[tokio::test]
    async fn shell_echo() {
        let shell = Shell::default();
        let rt = crate::soul::agent::Runtime::default();
        let result = shell
            .call(serde_json::json!({"command": "echo hello"}), &rt)
            .await;
        match result {
            crate::soul::message::ToolReturnValue::Ok { output, .. } => {
                assert!(output.contains("hello"));
            }
            _ => panic!("expected ok"),
        }
    }

    #[tokio::test]
    async fn shell_empty_command() {
        let shell = Shell::default();
        let rt = crate::soul::agent::Runtime::default();
        let result = shell.call(serde_json::json!({"command": ""}), &rt).await;
        assert!(
            matches!(result, crate::soul::message::ToolReturnValue::Error { .. })
        );
    }
}
