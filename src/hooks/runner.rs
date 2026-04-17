use serde_json::{Map, Value};

/// Result of a single hook execution.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub action: HookAction,
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}

impl Default for HookResult {
    fn default() -> Self {
        Self {
            action: HookAction::Allow,
            reason: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            timed_out: false,
        }
    }
}

/// Action returned by a hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookAction {
    Allow,
    Block,
}

/// Execute a single hook command. Fail-open: errors/timeouts -> allow.
#[tracing::instrument(level = "debug", skip(command, input_data))]
pub async fn run_hook(
    command: &str,
    input_data: &Map<String, Value>,
    timeout: u64,
    cwd: Option<&std::path::Path>,
) -> HookResult {
    let json_input = match serde_json::to_vec(input_data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to serialize hook input: {e}");
            return HookResult {
                action: HookAction::Allow,
                reason: format!("input serialization failed: {e}"),
                ..Default::default()
            };
        }
    };

    let mut cmd = tokio::process::Command::new("sh");
    crate::utils::subprocess_env::apply_to_tokio(
        &mut cmd,
        crate::utils::subprocess_env::get_clean_env(),
    );
    cmd.arg("-c").arg(command);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Hook spawn failed: {command}: {e}");
            return HookResult {
                action: HookAction::Allow,
                reason: format!("spawn failed: {e}"),
                stderr: e.to_string(),
                ..Default::default()
            };
        }
    };

    let stdin = child.stdin.take();
    let write_fut = async {
        if let Some(mut stdin) = stdin {
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stdin, &json_input).await;
        }
    };
    write_fut.await;

    let timeout_result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout),
        child.wait_with_output(),
    )
    .await;

    let output = match timeout_result {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::warn!("Hook failed: {command}: {e}");
            return HookResult {
                action: HookAction::Allow,
                reason: format!("execution failed: {e}"),
                stderr: e.to_string(),
                ..Default::default()
            };
        }
        Err(_) => {
            tracing::warn!("Hook timed out after {}s: {}", timeout, command);
            // child is killed automatically by kill_on_drop when the future is dropped
            return HookResult {
                action: HookAction::Allow,
                timed_out: true,
                reason: format!("timed out after {timeout}s"),
                ..Default::default()
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(0);

    // Exit 2 = block
    if exit_code == 2 {
        return HookResult {
            action: HookAction::Block,
            reason: stderr.trim().to_string(),
            stdout,
            stderr,
            exit_code: 2,
            timed_out: false,
        };
    }

    // Exit 0 + JSON stdout = structured decision
    if exit_code == 0 && !stdout.trim().is_empty() {
        if let Ok(raw) = serde_json::from_str::<Value>(&stdout) {
            if let Some(hook_output) = raw.get("hookSpecificOutput").and_then(|v| v.as_object()) {
                if hook_output
                    .get("permissionDecision")
                    .and_then(|v| v.as_str())
                    == Some("deny")
                {
                    return HookResult {
                        action: HookAction::Block,
                        reason: hook_output
                            .get("permissionDecisionReason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        stdout,
                        stderr,
                        exit_code: 0,
                        timed_out: false,
                    };
                }
            }
        }
    }

    HookResult {
        action: HookAction::Allow,
        stdout,
        stderr,
        exit_code,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_hook_allow_echo() {
        let input = Map::new();
        let result = run_hook("cat", &input, 5, None).await;
        assert_eq!(result.action, HookAction::Allow);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn run_hook_block_exit_2() {
        let input = Map::new();
        let result = run_hook("exit 2", &input, 5, None).await;
        assert_eq!(result.action, HookAction::Block);
        assert_eq!(result.exit_code, 2);
    }

    #[tokio::test]
    async fn run_hook_json_deny() {
        let input = Map::new();
        let result = run_hook(
            r#"echo '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}'"#,
            &input,
            5,
            None,
        )
        .await;
        assert_eq!(result.action, HookAction::Block);
        assert_eq!(result.reason, "no");
    }

    #[tokio::test]
    async fn run_hook_timeout_fail_open() {
        let input = Map::new();
        let result = run_hook("sleep 10", &input, 1, None).await;
        assert_eq!(result.action, HookAction::Allow);
        assert!(result.timed_out);
    }
}
