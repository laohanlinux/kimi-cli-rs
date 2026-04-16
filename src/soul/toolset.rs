use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

tokio::task_local! {
    #[doc = "Task-local context for the current tool call during tool execution."]
    pub static CURRENT_TOOL_CALL: crate::soul::message::ToolCall;
}

/// Returns the current tool call from task-local context, if any.
pub fn get_current_tool_call_or_none() -> Option<crate::soul::message::ToolCall> {
    CURRENT_TOOL_CALL.try_with(|tc| tc.clone()).ok()
}

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
    tools: Arc<tokio::sync::RwLock<HashMap<String, Arc<dyn Tool>>>>,
    hidden: HashSet<String>,
    hook_engine: Option<crate::hooks::engine::HookEngine>,
    deny_all: bool,
    mcp_servers: Arc<tokio::sync::RwLock<HashMap<String, crate::mcp::server::McpServerInfo>>>,
    mcp_loading_task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<crate::error::Result<()>>>>>,
    deferred_mcp_load: Arc<tokio::sync::Mutex<Option<(Vec<crate::config::McpConfig>, crate::soul::agent::Runtime)>>>,
    external_tools: Arc<tokio::sync::RwLock<HashMap<String, Arc<dyn Tool>>>>,
}

/// Strips Python module prefix from a tool name.
fn normalize_tool_name(name: &str) -> &str {
    if let Some(idx) = name.rfind(':') {
        &name[idx + 1..]
    } else {
        name
    }
}

impl KimiToolset {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            hidden: HashSet::new(),
            hook_engine: None,
            deny_all: false,
            mcp_servers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            mcp_loading_task: Arc::new(tokio::sync::Mutex::new(None)),
            deferred_mcp_load: Arc::new(tokio::sync::Mutex::new(None)),
            external_tools: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Creates a deny-all wrapper around an existing toolset.
    pub fn deny_all(source: &Self) -> Self {
        Self {
            tools: source.tools.clone(),
            hidden: source.hidden.clone(),
            hook_engine: None,
            deny_all: true,
            mcp_servers: source.mcp_servers.clone(),
            mcp_loading_task: source.mcp_loading_task.clone(),
            deferred_mcp_load: source.deferred_mcp_load.clone(),
            external_tools: source.external_tools.clone(),
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
        let keys: Vec<_> = {
            let rt = tokio::runtime::Handle::try_current();
            match rt {
                Ok(_) => {
                    // Best-effort blocking read of the keys.
                    self.tools.blocking_read().keys().cloned().collect()
                }
                Err(_) => Vec::new(),
            }
        };
        f.debug_struct("KimiToolset")
            .field("tools", &keys)
            .field("hidden", &self.hidden)
            .field("hook_engine", &self.hook_engine)
            .finish()
    }
}

impl KimiToolset {
    pub async fn add(&self, tool: Arc<dyn Tool>) {
        self.tools.write().await.insert(tool.name().to_string(), tool);
    }

    pub fn add_sync(&self, tool: Arc<dyn Tool>) {
        self.tools.blocking_write().insert(tool.name().to_string(), tool);
    }

    pub fn hide(&mut self, name: &str) {
        self.hidden.insert(name.to_string());
    }

    pub fn unhide(&mut self, name: &str) {
        self.hidden.remove(name);
    }

    /// Registers an external tool (e.g. from the web UI).
    pub async fn register_external_tool(&self, name: &str, tool: Arc<dyn Tool>) {
        self.external_tools.write().await.insert(name.to_string(), tool);
    }

    pub async fn find(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    /// Loads built-in tools by name into the toolset.
    #[tracing::instrument(level = "debug", skip(self, _deps))]
    pub async fn load_tools(
        &mut self,
        tools: &[String],
        _deps: HashMap<String, serde_json::Value>,
    ) -> crate::error::Result<()> {
        for name in tools {
            let simple_name = normalize_tool_name(name);
            match simple_name {
                "ReadFile" => self.add(Arc::new(crate::tools::file::ReadFile)).await,
                "WriteFile" => self.add(Arc::new(crate::tools::file::WriteFile)).await,
                "StrReplaceFile" => self.add(Arc::new(crate::tools::file::StrReplaceFile)).await,
                "Glob" => self.add(Arc::new(crate::tools::file::Glob)).await,
                "Grep" => self.add(Arc::new(crate::tools::file::Grep)).await,
                "ReadMediaFile" => self.add(Arc::new(crate::tools::file::ReadMediaFile)).await,
                "Shell" => self.add(Arc::new(crate::tools::shell::Shell::default())).await,
                "SearchWeb" => self.add(Arc::new(crate::tools::web::SearchWeb)).await,
                "FetchURL" => self.add(Arc::new(crate::tools::web::FetchUrl)).await,
                "AskUserQuestion" => self.add(Arc::new(crate::tools::ask_user::AskUserQuestion)).await,
                "EnterPlanMode" => self.add(Arc::new(crate::tools::plan::EnterPlanMode)).await,
                "ExitPlanMode" => self.add(Arc::new(crate::tools::plan::ExitPlanMode)).await,
                "Think" => self.add(Arc::new(crate::tools::think::Think)).await,
                "SetTodoList" => self.add(Arc::new(crate::tools::todo::SetTodoList)).await,
                "SendDMail" => self.add(Arc::new(crate::tools::dmail::SendDMail)).await,
                "TaskOutput" => self.add(Arc::new(crate::tools::background::TaskOutput)).await,
                "TaskList" => self.add(Arc::new(crate::tools::background::TaskList)).await,
                "TaskStop" => self.add(Arc::new(crate::tools::background::TaskStop)).await,
                "Agent" => {
                    // Agent tool is added separately by load_agent because it requires a Runtime.
                }
                unknown => {
                    tracing::warn!("Unknown tool requested: {}", unknown);
                }
            }
        }
        Ok(())
    }

    /// Loads MCP tools from the provided server configurations.
    #[tracing::instrument(level = "debug", skip(self, runtime))]
    pub async fn load_mcp_tools(
        &mut self,
        configs: Vec<crate::config::McpConfig>,
        runtime: &crate::soul::agent::Runtime,
        in_background: bool,
    ) -> crate::error::Result<()> {
        if configs.is_empty() {
            tracing::debug!("No MCP configs provided, skipping MCP tool loading");
            return Ok(());
        }

        let timeout_ms = runtime.config.mcp.client.tool_call_timeout_ms;
        let mcp_servers = self.mcp_servers.clone();
        let tools_map = self.tools.clone();

        let fut = async move {
            let mut failed = Vec::new();

            {
                let mut servers_guard = mcp_servers.write().await;
                for config in &configs {
                    for (server_name, _server_config) in &config.servers {
                        if servers_guard.contains_key(server_name) {
                            continue;
                        }
                        servers_guard.insert(
                            server_name.clone(),
                            crate::mcp::server::McpServerInfo::new(),
                        );
                    }
                }
            }

            for config in configs {
                for (server_name, server_config) in config.servers {
                    let mut servers_guard = mcp_servers.write().await;
                    let info = match servers_guard.get_mut(&server_name) {
                        Some(i) if i.status == crate::mcp::server::McpServerStatus::Pending => i,
                        _ => continue,
                    };

                    info.status = crate::mcp::server::McpServerStatus::Connecting;
                    drop(servers_guard);

                    let conn_result = match &server_config {
                        crate::config::McpServerConfig::Stdio { command, args, env } => {
                            crate::mcp::client::connect_stdio(command, args, env).await
                        }
                        crate::config::McpServerConfig::Http { url, headers, .. } => {
                            crate::mcp::client::connect_http(url, headers).await
                        }
                    };

                    let mut servers_guard = mcp_servers.write().await;
                    let info = servers_guard.get_mut(&server_name).unwrap();

                    match conn_result {
                        Ok(conn) => {
                            match conn.peer.list_all_tools().await {
                                Ok(tools) => {
                                    let mut tool_names = Vec::new();
                                    for tool in &tools {
                                        let mcp_tool = crate::mcp::tool::McpTool::new(
                                            server_name.clone(),
                                            tool,
                                            conn.peer.clone(),
                                            timeout_ms,
                                        );
                                        tool_names.push(mcp_tool.name().to_string());
                                        tools_map.write().await.insert(
                                            mcp_tool.name().to_string(),
                                            Arc::new(mcp_tool),
                                        );
                                    }
                                    info.status = crate::mcp::server::McpServerStatus::Connected;
                                    info.connection = Some(conn);
                                    info.tool_names = tool_names;
                                    tracing::info!("Connected MCP server: {server_name}");
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to list tools from MCP server: {server_name}, error: {e}"
                                    );
                                    info.status = crate::mcp::server::McpServerStatus::Failed;
                                    conn.cancel();
                                    failed.push(server_name.clone());
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to connect MCP server: {server_name}, error: {e}"
                            );
                            info.status = crate::mcp::server::McpServerStatus::Failed;
                            failed.push(server_name.clone());
                        }
                    }
                }
            }

            if !failed.is_empty() {
                return Err(crate::error::KimiCliError::McpRuntime(format!(
                    "Failed to connect MCP servers: {failed:?}"
                )));
            }
            Ok(())
        };

        if in_background {
            let handle = tokio::spawn(fut);
            *self.mcp_loading_task.lock().await = Some(handle);
        } else {
            fut.await?;
        }

        Ok(())
    }

    /// Defers MCP tool loading to a background task.
    pub async fn defer_mcp_tool_loading(
        &mut self,
        configs: Vec<crate::config::McpConfig>,
        runtime: &crate::soul::agent::Runtime,
    ) {
        if configs.is_empty() {
            return;
        }
        let runtime = runtime.clone();
        *self.deferred_mcp_load.lock().await = Some((configs, runtime));
    }

    /// Returns true when MCP loading is configured but has not started yet.
    pub fn has_deferred_mcp_tools(&self) -> bool {
        self.deferred_mcp_load.blocking_lock().is_some()
    }

    /// Starts any deferred MCP loading in the background.
    pub async fn start_deferred_mcp_tool_loading(&mut self) -> bool {
        let mut deferred = self.deferred_mcp_load.lock().await;
        if deferred.is_none() {
            return false;
        }
        let mcp_loading = self.mcp_loading_task.lock().await;
        let servers = self.mcp_servers.read().await;
        if mcp_loading.is_some() || !servers.is_empty() {
            *deferred = None;
            return false;
        }
        drop(mcp_loading);
        drop(servers);

        let (configs, runtime) = deferred.take().unwrap();
        drop(deferred);

        let _ = self.load_mcp_tools(configs, &runtime, true).await;
        true
    }

    /// Starts background MCP tool loading if configs are available.
    pub fn start_background_mcp_loading(&mut self) -> bool {
        let clone = self.clone();
        tokio::spawn(async move {
            let mut toolset = clone;
            let _ = toolset.start_deferred_mcp_tool_loading().await;
        });
        true
    }

    /// Waits for any background MCP tool loading to complete.
    pub async fn wait_for_background_mcp_loading(&mut self) {
        let task = self.mcp_loading_task.lock().await.take();
        if let Some(handle) = task {
            let _ = handle.await;
        }
    }

    /// Returns true if the background MCP tool-loading task is still running.
    pub fn has_pending_mcp_tools(&self) -> bool {
        if let Ok(guard) = self.mcp_loading_task.try_lock() {
            guard.as_ref().map_or(false, |h| !h.is_finished())
        } else {
            true
        }
    }

    /// Returns a read-only snapshot of current MCP startup state.
    pub fn mcp_status_snapshot(&self) -> Option<crate::mcp::server::McpStatusSnapshot> {
        let servers = self.mcp_servers.blocking_read();
        if servers.is_empty() {
            return None;
        }

        let snapshots: Vec<crate::mcp::server::McpServerSnapshot> = servers
            .iter()
            .map(|(name, info)| crate::mcp::server::McpServerSnapshot {
                name: name.clone(),
                status: info.status.to_string(),
                tools: info.tool_names.clone(),
            })
            .collect();

        let connected = snapshots
            .iter()
            .filter(|s| s.status == "connected")
            .count();
        let tools = snapshots.iter().map(|s| s.tools.len()).sum();

        Some(crate::mcp::server::McpStatusSnapshot {
            loading: self.has_pending_mcp_tools(),
            connected,
            total: snapshots.len(),
            tools,
            servers: snapshots,
        })
    }

    /// Returns the underlying MCP server map.
    pub fn mcp_servers(&self) -> Arc<tokio::sync::RwLock<HashMap<String, crate::mcp::server::McpServerInfo>>> {
        self.mcp_servers.clone()
    }

    /// Returns a mutable reference to the hook engine option.
    pub fn hook_engine_mut(&mut self) -> &mut Option<crate::hooks::engine::HookEngine> {
        &mut self.hook_engine
    }

    /// Sets the hook engine on the toolset.
    pub fn set_hook_engine(&mut self, engine: crate::hooks::engine::HookEngine) {
        self.hook_engine = Some(engine);
    }

    /// Returns the underlying tool map.
    pub async fn tools(&self) -> HashMap<String, Arc<dyn Tool>> {
        self.tools.read().await.clone()
    }

    /// Returns the underlying tool map (blocking).
    pub fn tools_sync(&self) -> HashMap<String, Arc<dyn Tool>> {
        self.tools.blocking_read().clone()
    }

    /// Looks up a tool and executes it.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn handle(
        &self,
        tool_call: &crate::soul::message::ToolCall,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolResult {
        if self.deny_all {
            return crate::soul::message::ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: crate::soul::message::ToolReturnValue::Error {
                    error: "Tool calls are disabled for side questions. Answer with text only.".into(),
                },
            };
        }

        let tool = {
            let guard = self.tools.read().await;
            guard.get(&tool_call.name).cloned()
        };
        let tool = tool.or_else(|| {
            let guard = self.external_tools.blocking_read();
            guard.get(&tool_call.name).cloned()
        });

        let Some(tool) = tool else {
            return crate::soul::message::ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: crate::soul::message::ToolReturnValue::Error {
                    error: format!("Tool '{}' not found", tool_call.name),
                },
            };
        };

        let args = tool_call.arguments.clone();

        // PreToolUse hook with rich event data.
        if let Some(ref engine) = self.hook_engine {
            let pre_data = serde_json::json!({
                "tool_name": tool_call.name,
                "tool_call_id": tool_call.id,
                "tool_input": args,
            });
            match engine.trigger("PreToolUse", &tool_call.name, pre_data).await {
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
        let result = CURRENT_TOOL_CALL.scope(tool_call.clone(), async {
            tool.call(args, runtime).await
        }).await;
        let elapsed = start.elapsed();
        tracing::info!(tool = %tool_call.name, ?elapsed, "tool executed");

        // Post-tool-use hooks.
        if let Some(ref engine) = self.hook_engine {
            let engine = engine.clone();
            let name = tool_call.name.clone();
            let output_preview = match &result {
                crate::soul::message::ToolReturnValue::Ok { output, .. } => output.chars().take(2000).collect::<String>(),
                crate::soul::message::ToolReturnValue::Error { error } => error.chars().take(2000).collect::<String>(),
                crate::soul::message::ToolReturnValue::Parts { parts } => {
                    parts.iter().map(|p| match p {
                        crate::soul::message::ContentPart::Text { text } => text.as_str(),
                        crate::soul::message::ContentPart::Think { thought } => thought.as_str(),
                        _ => "",
                    }).collect::<String>().chars().take(2000).collect::<String>()
                }
            };
            let is_error = matches!(result, crate::soul::message::ToolReturnValue::Error { .. });
            tokio::spawn(async move {
                if is_error {
                    let _ = engine.trigger("PostToolUseFailure", &name, serde_json::json!({"tool_output": output_preview})).await;
                }
                let _ = engine.trigger("PostToolUse", &name, serde_json::json!({"tool_output": output_preview})).await;
            });
        }

        crate::soul::message::ToolResult {
            tool_call_id: tool_call.id.clone(),
            return_value: result,
        }
    }

    /// Cleans up any resources held by the toolset.
    pub async fn cleanup(&mut self) {
        *self.deferred_mcp_load.lock().await = None;

        if let Some(handle) = self.mcp_loading_task.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }

        let mut servers = self.mcp_servers.write().await;
        for (_name, info) in servers.iter_mut() {
            if let Some(conn) = info.connection.take() {
                conn.cancel();
            }
        }
        servers.clear();
    }
}
