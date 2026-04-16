use async_trait::async_trait;
use std::time::Duration;

use rmcp::model::CallToolRequestParams;

/// A tool backed by an MCP server.
pub struct McpTool {
    server_name: String,
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
    peer: rmcp::service::Peer<rmcp::service::RoleClient>,
    timeout: Duration,
}

impl McpTool {
    pub fn new(
        server_name: String,
        mcp_tool: &rmcp::model::Tool,
        peer: rmcp::service::Peer<rmcp::service::RoleClient>,
        timeout_ms: usize,
    ) -> Self {
        let schema = serde_json::Value::Object(mcp_tool.input_schema.as_ref().clone());
        let description = format!(
            "This is an MCP (Model Context Protocol) tool from MCP server `{server_name}`.\n\n{}",
            mcp_tool.description.as_deref().unwrap_or("No description provided.")
        );
        Self {
            server_name,
            name: mcp_tool.name.to_string(),
            description,
            parameters_schema: schema,
            peer,
            timeout: Duration::from_millis(timeout_ms as u64),
        }
    }
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("server_name", &self.server_name)
            .field("name", &self.name)
            .finish()
    }
}

#[async_trait]
impl crate::soul::toolset::Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.parameters_schema.clone()
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let args_obj = match arguments {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        let mut params = CallToolRequestParams::new(self.name.clone());
        params.arguments = Some(args_obj);

        let result = tokio::time::timeout(self.timeout, self.peer.call_tool(params)).await;

        match result {
            Ok(Ok(tool_result)) => crate::mcp::result::convert_mcp_result(&tool_result),
            Ok(Err(e)) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("timeout") || msg.contains("timed out") {
                    crate::soul::message::ToolReturnValue::Error {
                        error: format!(
                            "Timeout while calling MCP tool `{}`. \
                             You may explain to the user that the timeout config is set too low.",
                            self.name
                        ),
                    }
                } else {
                    tracing::error!("MCP tool call failed: {}: {}", self.name, e);
                    crate::soul::message::ToolReturnValue::Error {
                        error: format!("MCP tool call failed: {e}"),
                    }
                }
            }
            Err(_) => crate::soul::message::ToolReturnValue::Error {
                error: format!(
                    "Timeout while calling MCP tool `{}`. \
                     You may explain to the user that the timeout config is set too low.",
                    self.name
                ),
            },
        }
    }
}
