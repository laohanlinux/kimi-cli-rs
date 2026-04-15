use regex::Regex;
use std::collections::HashMap;

/// Action returned by a hook trigger.
#[derive(Debug, Clone)]
pub enum HookAction {
    Allow,
    Block { reason: String },
}

/// Engine that executes registered hooks.
#[derive(Debug, Clone, Default)]
pub struct HookEngine {
    hooks: Vec<crate::config::HookDef>,
}

impl HookEngine {
    /// Creates a hook engine from config definitions.
    pub fn from_defs(hooks: Vec<crate::config::HookDef>) -> Self {
        Self { hooks }
    }

    /// Triggers the named hook.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn trigger(
        &self,
        hook_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> crate::error::Result<HookAction> {
        for hook in &self.hooks {
            if !event_matches(&hook.event, hook_name) {
                continue;
            }
            if let Some(ref matcher) = hook.matcher {
                if !wildcard_match(matcher, tool_name) {
                    continue;
                }
            }

            tracing::debug!(
                hook = %hook.event,
                command = %hook.command,
                tool = %tool_name,
                "executing hook"
            );

            let result = self.execute_hook(hook, tool_name, &arguments).await?;
            if let HookAction::Block { ref reason } = result {
                tracing::info!(tool = %tool_name, reason = %reason, "hook blocked tool");
                return Ok(result);
            }
        }

        Ok(HookAction::Allow)
    }

    async fn execute_hook(
        &self,
        hook: &crate::config::HookDef,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> crate::error::Result<HookAction> {
        let cmd = hook.command.trim();

        // Treat "block" as a built-in blocking action.
        if cmd.eq_ignore_ascii_case("block") {
            return Ok(HookAction::Block {
                reason: format!("blocked by {} hook for '{}'", hook.event, tool_name),
            });
        }

        // Simple command execution via shell.
        let output = tokio::time::timeout(
            tokio::time::Duration::from_secs(hook.timeout),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .env("KIMI_HOOK_EVENT", &hook.event)
                .env("KIMI_HOOK_TOOL", tool_name)
                .env("KIMI_HOOK_ARGS", arguments.to_string())
                .output(),
        )
        .await
        .map_err(|_| crate::error::KimiCliError::Generic("hook command timed out".into()))?
        .map_err(|e| crate::error::KimiCliError::Generic(format!("hook command failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            return Ok(HookAction::Block {
                reason: format!("hook exited with error: {stderr}"),
            });
        }

        if stdout.eq_ignore_ascii_case("block") {
            return Ok(HookAction::Block {
                reason: format!("hook returned block for '{}'", tool_name),
            });
        }

        Ok(HookAction::Allow)
    }
}

fn event_matches(pattern: &str, event: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    wildcard_match(pattern, event)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == text;
    }
    let regex_str = regex::escape(pattern).replace(r"\*", ".*");
    match Regex::new(&format!("^{}$", regex_str)) {
        Ok(re) => re.is_match(text),
        Err(_) => pattern == text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_engine_default_allows() {
        let engine = HookEngine::default();
        let rt = tokio_test::block_on(engine.trigger("PreToolUse", "Shell", serde_json::Value::Null));
        assert!(matches!(rt.unwrap(), HookAction::Allow));
    }

    #[test]
    fn hook_engine_block_builtin() {
        let engine = HookEngine::from_defs(vec![crate::config::HookDef {
            event: "PreToolUse".into(),
            command: "block".into(),
            matcher: Some("Shell".into()),
            timeout: 30,
        }]);
        let rt = tokio_test::block_on(engine.trigger("PreToolUse", "Shell", serde_json::Value::Null));
        assert!(matches!(rt.unwrap(), HookAction::Block { .. }));
    }

    #[test]
    fn hook_engine_wildcard_event() {
        let engine = HookEngine::from_defs(vec![crate::config::HookDef {
            event: "*".into(),
            command: "block".into(),
            matcher: Some("ReadFile".into()),
            timeout: 30,
        }]);
        let rt = tokio_test::block_on(engine.trigger("PostToolUse", "ReadFile", serde_json::Value::Null));
        assert!(matches!(rt.unwrap(), HookAction::Block { .. }));
    }
}
