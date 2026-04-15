/// A node within a skill flow.
#[derive(Debug, Clone)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
}

/// An edge connecting two flow nodes.
#[derive(Debug, Clone)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

/// Parsed skill flow diagram.
#[derive(Debug, Clone, Default)]
pub struct Flow {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

/// Error during flow parsing.
#[derive(Debug, Clone)]
pub struct FlowError {
    pub message: String,
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FlowError {}
