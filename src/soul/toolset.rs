use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Trait implemented by every tool.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    /// Executes the tool with the given arguments and runtime context.
    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue;
}

/// Central registry and executor for all tools.
#[derive(Clone)]
pub struct KimiToolset {
    tools: HashMap<String, Arc<dyn Tool>>,
    hidden: HashSet<String>,
    hook_engine: Option<crate::hooks::engine::HookEngine>,
}

impl KimiToolset {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            hidden: HashSet::new(),
            hook_engine: None,
        }
    }
}

impl Default for KimiToolset {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for KimiToolset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KimiToolset")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .field("hidden", &self.hidden)
            .field("hook_engine", &self.hook_engine)
            .finish()
    }
}

impl KimiToolset {
    pub fn add(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn hide(&mut self, name: &str) {
        self.hidden.insert(name.to_string());
    }

    pub fn find(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Loads built-in tools by name into the toolset.
    #[tracing::instrument(level = "debug", skip(self, _deps))]
    pub fn load_tools(
        &mut self,
        tools: &[String],
        _deps: HashMap<String, serde_json::Value>,
    ) -> crate::error::Result<()> {
        for name in tools {
            match name.as_str() {
                "ReadFile" => self.add(Arc::new(crate::tools::file::ReadFile)),
                "WriteFile" => self.add(Arc::new(crate::tools::file::WriteFile)),
                "StrReplaceFile" => self.add(Arc::new(crate::tools::file::StrReplaceFile)),
                "Glob" => self.add(Arc::new(crate::tools::file::Glob)),
                "Grep" => self.add(Arc::new(crate::tools::file::Grep)),
                "ReadMediaFile" => self.add(Arc::new(crate::tools::file::ReadMediaFile)),
                "Shell" => self.add(Arc::new(crate::tools::shell::Shell::default())),
                "SearchWeb" => self.add(Arc::new(crate::tools::web::SearchWeb)),
                "FetchURL" => self.add(Arc::new(crate::tools::web::FetchUrl)),
                "AskUserQuestion" => self.add(Arc::new(crate::tools::ask_user::AskUserQuestion)),
                "EnterPlanMode" => self.add(Arc::new(crate::tools::plan::EnterPlanMode)),
                "ExitPlanMode" => self.add(Arc::new(crate::tools::plan::ExitPlanMode)),
                "Think" => self.add(Arc::new(crate::tools::think::Think)),
                "SetTodoList" => self.add(Arc::new(crate::tools::todo::SetTodoList)),
                "SendDMail" => self.add(Arc::new(crate::tools::dmail::SendDMail)),
                "TaskOutput" => self.add(Arc::new(crate::tools::background::TaskOutput)),
                "TaskList" => self.add(Arc::new(crate::tools::background::TaskList)),
                "TaskStop" => self.add(Arc::new(crate::tools::background::TaskStop)),
                unknown => {
                    tracing::warn!("Unknown tool requested: {}", unknown);
                }
            }
        }
        Ok(())
    }

    /// Loads MCP tools from the provided server configurations.
    #[tracing::instrument(level = "debug", skip(self, _runtime))]
    pub async fn load_mcp_tools(
        &mut self,
        configs: Vec<crate::config::McpConfig>,
        _runtime: &crate::soul::agent::Runtime,
        _in_background: bool,
    ) -> crate::error::Result<()> {
        if configs.is_empty() {
            tracing::debug!("No MCP configs provided, skipping MCP tool loading");
            return Ok(());
        }
        tracing::info!(
            count = configs.len(),
            timeout_ms = _runtime.config.mcp.client.tool_call_timeout_ms,
            "MCP tool loading is not yet fully implemented in the Rust port"
        );
        Ok(())
    }

    /// Defers MCP tool loading to a background task.
    pub fn defer_mcp_tool_loading(
        &mut self,
        configs: Vec<crate::config::McpConfig>,
        runtime: &crate::soul::agent::Runtime,
    ) {
        if configs.is_empty() {
            return;
        }
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let mut toolset = KimiToolset::new();
            if let Err(e) = toolset.load_mcp_tools(configs, &runtime, true).await {
                tracing::warn!("Deferred MCP tool loading failed: {}", e);
            }
        });
    }

    /// Sets the hook engine on the toolset.
    pub fn set_hook_engine(&mut self, engine: crate::hooks::engine::HookEngine) {
        self.hook_engine = Some(engine);
    }

    /// Starts background MCP tool loading if configs are available.
    pub fn start_background_mcp_loading(&mut self) -> bool {
        tracing::debug!("Background MCP loading requested (stub)");
        false
    }

    /// Waits for any background MCP tool loading to complete.
    pub async fn wait_for_background_mcp_loading(&mut self) {
        // no-op until real MCP client implementation
    }

    /// Returns the underlying tool map.
    pub fn tools(&self) -> &HashMap<String, Arc<dyn Tool>> {
        &self.tools
    }

    /// Looks up a tool and executes it.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn handle(
        &self,
        tool_call: &crate::soul::message::ToolCall,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolResult {
        let Some(tool) = self.tools.get(&tool_call.name) else {
            return crate::soul::message::ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: crate::soul::message::ToolReturnValue::Error {
                    error: format!("Tool '{}' not found", tool_call.name),
                },
            };
        };

        let args = tool_call.arguments.clone();

        // PreToolUse hook.
        if let Some(ref engine) = self.hook_engine {
            match engine.trigger("PreToolUse", &tool_call.name, args.clone()).await {
                Ok(crate::hooks::engine::HookAction::Block { reason }) => {
                    return crate::soul::message::ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        return_value: crate::soul::message::ToolReturnValue::Error {
                            error: format!("Blocked by hook: {reason}"),
                        },
                    };
                }
                _ => {}
            }
        }

        let start = std::time::Instant::now();
        let result = tool.call(args, runtime).await;
        let elapsed = start.elapsed();
        tracing::info!(tool = %tool_call.name, ?elapsed, "tool executed");

        if let Some(ref engine) = self.hook_engine {
            let engine = engine.clone();
            let name = tool_call.name.clone();
            tokio::spawn(async move {
                let _ = engine.trigger("PostToolUse", &name, serde_json::json!({})).await;
            });
        }

        crate::soul::message::ToolResult {
            tool_call_id: tool_call.id.clone(),
            return_value: result,
        }
    }
}
