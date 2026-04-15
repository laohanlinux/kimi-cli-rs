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

    pub fn load_tools(
        &mut self,
        _tools: &[String],
        _deps: HashMap<String, serde_json::Value>,
    ) -> crate::error::Result<()> {
        // TODO: implement full tool loading
        Ok(())
    }

    pub async fn load_mcp_tools(
        &mut self,
        _configs: Vec<crate::config::McpConfig>,
        _runtime: &crate::soul::agent::Runtime,
        _in_background: bool,
    ) -> crate::error::Result<()> {
        // TODO: implement MCP tool loading
        Ok(())
    }

    pub fn defer_mcp_tool_loading(
        &mut self,
        _configs: Vec<crate::config::McpConfig>,
        _runtime: &crate::soul::agent::Runtime,
    ) {
        // TODO: implement deferred MCP loading
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
