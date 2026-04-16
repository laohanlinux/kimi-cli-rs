/// Lifecycle status of an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    Pending,
    Connecting,
    Connected,
    Failed,
    Unauthorized,
}

impl std::fmt::Display for McpServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpServerStatus::Pending => write!(f, "pending"),
            McpServerStatus::Connecting => write!(f, "connecting"),
            McpServerStatus::Connected => write!(f, "connected"),
            McpServerStatus::Failed => write!(f, "failed"),
            McpServerStatus::Unauthorized => write!(f, "unauthorized"),
        }
    }
}

/// Information about a connected (or attempted) MCP server.
#[derive(Debug)]
pub struct McpServerInfo {
    pub status: McpServerStatus,
    pub connection: Option<crate::mcp::client::McpConnection>,
    pub tool_names: Vec<String>,
}

impl McpServerInfo {
    pub fn new() -> Self {
        Self {
            status: McpServerStatus::Pending,
            connection: None,
            tool_names: Vec::new(),
        }
    }
}

impl Default for McpServerInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Read-only snapshot of an MCP server for UI display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpServerSnapshot {
    pub name: String,
    pub status: String,
    pub tools: Vec<String>,
}

/// Overall MCP status snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpStatusSnapshot {
    pub loading: bool,
    pub connected: usize,
    pub total: usize,
    pub tools: usize,
    pub servers: Vec<McpServerSnapshot>,
}
