pub mod d2;
pub mod mermaid;

use std::collections::HashMap;

/// Kind of a flow node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowNodeKind {
    Begin,
    End,
    Task,
    Decision,
}

impl std::fmt::Display for FlowNodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowNodeKind::Begin => write!(f, "begin"),
            FlowNodeKind::End => write!(f, "end"),
            FlowNodeKind::Task => write!(f, "task"),
            FlowNodeKind::Decision => write!(f, "decision"),
        }
    }
}

/// A node within a skill flow.
#[derive(Debug, Clone)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
    pub kind: FlowNodeKind,
}

impl PartialEq for FlowNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.label == other.label && self.kind == other.kind
    }
}

/// An edge connecting two flow nodes.
#[derive(Debug, Clone)]
pub struct FlowEdge {
    pub src: String,
    pub dst: String,
    pub label: Option<String>,
}

/// Parsed skill flow diagram.
#[derive(Debug, Clone, Default)]
pub struct Flow {
    pub nodes: HashMap<String, FlowNode>,
    pub outgoing: HashMap<String, Vec<FlowEdge>>,
    pub begin_id: Option<String>,
    pub end_id: Option<String>,
}

/// Error during flow parsing.
#[derive(Debug, Clone)]
pub struct FlowError {
    pub message: String,
}

impl FlowError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FlowError {}

/// Validates the flow has exactly one begin and one end node.
pub fn validate_flow(
    nodes: &HashMap<String, FlowNode>,
    outgoing: &HashMap<String, Vec<FlowEdge>>,
) -> Result<(Option<String>, Option<String>), FlowError> {
    let mut begin_ids = Vec::new();
    let mut end_ids = Vec::new();

    for (id, node) in nodes {
        match node.kind {
            FlowNodeKind::Begin => begin_ids.push(id.clone()),
            FlowNodeKind::End => end_ids.push(id.clone()),
            _ => {}
        }
    }

    if begin_ids.is_empty() {
        return Err(FlowError::new("Flow has no begin node"));
    }
    if begin_ids.len() > 1 {
        return Err(FlowError::new(format!(
            "Flow has multiple begin nodes: {}",
            begin_ids.join(", ")
        )));
    }
    if end_ids.is_empty() {
        return Err(FlowError::new("Flow has no end node"));
    }
    if end_ids.len() > 1 {
        return Err(FlowError::new(format!(
            "Flow has multiple end nodes: {}",
            end_ids.join(", ")
        )));
    }

    // Check reachability from begin to all nodes
    if let Some(begin) = begin_ids.first() {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![begin.to_string()];
        while let Some(curr) = stack.pop() {
            if !visited.insert(curr.clone()) {
                continue;
            }
            for edge in outgoing.get(curr.as_str()).unwrap_or(&Vec::new()) {
                stack.push(edge.dst.clone());
            }
        }

        let unreachable: Vec<_> = nodes
            .keys()
            .filter(|k| !visited.contains(k.as_str()))
            .cloned()
            .collect();
        if !unreachable.is_empty() {
            return Err(FlowError::new(format!(
                "Unreachable nodes: {}",
                unreachable.join(", ")
            )));
        }
    }

    Ok((begin_ids.into_iter().next(), end_ids.into_iter().next()))
}
