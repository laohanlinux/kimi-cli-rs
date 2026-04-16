use async_trait::async_trait;

const MAX_FOREGROUND_TIMEOUT: u64 = 60 * 60; // 1 hour

/// Delegates tasks to a focused subagent instance.
#[derive(Debug, Clone)]
pub struct Agent {
    description: String,
}

impl Agent {
    pub fn new(runtime: crate::soul::agent::Runtime) -> Self {
        let description = Self::build_description(&runtime);
        Self { description }
    }

    fn build_description(runtime: &crate::soul::agent::Runtime) -> String {
        let mut lines = vec![
            "Start a subagent instance to work on a focused task.".into(),
            "".into(),
            "The Agent tool can either create a new subagent instance or resume an existing one by `agent_id`.".into(),
            "Each instance keeps its own context history under the current session, so repeated use of the same".into(),
            "instance can preserve previous findings and work.".into(),
            "".into(),
            "**Available Built-in Agent Types**".into(),
            "".into(),
        ];

        let rt = runtime.labor_market.blocking_read();
        for (name, type_def) in &rt.builtin_types {
            let tool_names = if type_def.tool_policy.mode == crate::subagents::labor_market::ToolPolicyMode::Inherit {
                "*".into()
            } else if type_def.tool_policy.tools.is_empty() {
                "(none)".into()
            } else {
                type_def.tool_policy.tools.join(", ")
            };
            let model = type_def.default_model.as_deref().unwrap_or("inherit");
            let background = if type_def.supports_background { "yes" } else { "no" };
            let when = if type_def.when_to_use.is_empty() {
                String::new()
            } else {
                format!(" When to use: {}", Self::normalize_summary(&type_def.when_to_use))
            };
            lines.push(format!(
                "- `{name}`: {description} (Tools: {tool_names}, Model: {model}, Background: {background}).{when}",
                name = name,
                description = type_def.description,
                tool_names = tool_names,
                model = model,
                background = background,
                when = when,
            ));
        }

        lines.extend_from_slice(&[
            "".into(),
            "**Usage**".into(),
            "".into(),
            "- Always provide a short `description` (3-5 words).".into(),
            "- Use `subagent_type` to select a built-in agent type. If omitted, `coder` is used.".into(),
            "- Use `model` when you need to override the built-in type's default model or the parent agent's current model.".into(),
            "- Use `resume` when you want to continue an existing instance instead of starting a new one.".into(),
            "- If an existing subagent already has relevant context or the task is a continuation of its prior work, prefer `resume` over creating a new instance.".into(),
            "- Default to foreground execution. Use `run_in_background=true` only when the task can continue independently, you do not need the result immediately, and there is a clear benefit to returning control before it finishes.".into(),
            "- Be explicit about whether the subagent should write code or only do research.".into(),
            "- The subagent result is only visible to you. If the user should see it, summarize it yourself.".into(),
        ]);

        lines.join("\n")
    }

    fn normalize_summary(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[async_trait]
impl crate::soul::toolset::Tool for Agent {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "subagent_type": {
                    "type": "string",
                    "default": "coder",
                    "description": "The built-in agent type to use. Defaults to `coder`."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override. Selection priority is: this parameter, then the built-in type default model, then the parent agent's current model."
                },
                "resume": {
                    "type": "string",
                    "description": "Optional agent ID to resume instead of creating a new instance."
                },
                "run_in_background": {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether to run the agent in the background."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds for the agent task. Foreground: no default timeout (runs until completion), max 3600s (1hr). Background: default from config (15min), max 3600s (1hr)."
                }
            },
            "required": ["description", "prompt"]
        })
    }

    #[tracing::instrument(level = "info", skip(self, arguments))]
    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if runtime.role != "root" {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Subagents cannot launch other subagents.".into(),
            };
        }

        let description = arguments
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let prompt = arguments
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let subagent_type = arguments
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("coder")
            .to_string();
        let model = arguments.get("model").and_then(|v| v.as_str()).map(String::from);
        let resume = arguments.get("resume").and_then(|v| v.as_str()).map(String::from);
        let run_in_background = arguments
            .get("run_in_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let timeout = arguments.get("timeout").and_then(|v| v.as_u64());

        if let Some(ref alias) = model {
            if !runtime.config.models.contains_key(alias) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Unknown model alias: {alias}"),
                };
            }
        }

        if run_in_background {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Background subagent execution is not yet implemented in the Rust port.".into(),
            };
        }

        let req = crate::subagents::runner::ForegroundRunRequest {
            description,
            prompt,
            requested_type: subagent_type,
            model,
            resume,
        };

        let runner = crate::subagents::runner::ForegroundSubagentRunner::new(runtime.clone());

        let result = if let Some(t) = timeout {
            let t = t.min(MAX_FOREGROUND_TIMEOUT);
            match tokio::time::timeout(tokio::time::Duration::from_secs(t), runner.run(&req)).await {
                Ok(r) => r,
                Err(_) => crate::soul::message::ToolReturnValue::Error {
                    error: format!("Agent timed out after {t}s."),
                },
            }
        } else {
            runner.run(&req).await
        };

        result
    }
}
