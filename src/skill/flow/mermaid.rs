use crate::skill::flow::{Flow, FlowEdge, FlowError, FlowNode, FlowNodeKind, validate_flow};
use std::collections::HashMap;

/// Internal node specification.
#[derive(Debug, Clone)]
struct NodeSpec {
    node_id: String,
    label: Option<String>,
}

/// Internal node definition with explicit flag.
#[derive(Debug, Clone)]
struct NodeDef {
    node: FlowNode,
    explicit: bool,
}

/// Parses a Mermaid flowchart into a Flow.
#[tracing::instrument(level = "debug")]
pub fn parse_mermaid_flowchart(text: &str) -> Result<Flow, FlowError> {
    let mut nodes: HashMap<String, NodeDef> = HashMap::new();
    let mut outgoing: HashMap<String, Vec<FlowEdge>> = HashMap::new();

    for (line_no, raw_line) in text.lines().enumerate() {
        let line_no = line_no + 1;
        let stripped = strip_comment(raw_line);
        let line = stripped.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if is_header(line) {
            continue;
        }
        if is_style_line(line) {
            continue;
        }
        let line = strip_style_tokens(line);

        if let Some((src_spec, label, dst_spec)) = try_parse_edge_line(&line, line_no) {
            let src_node = add_node(&mut nodes, &src_spec, line_no)?;
            let dst_node = add_node(&mut nodes, &dst_spec, line_no)?;
            let edge = FlowEdge {
                src: src_node.id,
                dst: dst_node.id,
                label,
            };
            outgoing.entry(edge.src.clone()).or_default().push(edge.clone());
            outgoing.entry(edge.dst.clone()).or_default();
            continue;
        }

        if let Some(node_spec) = try_parse_node_line(&line, line_no) {
            add_node(&mut nodes, &node_spec, line_no)?;
        }
    }

    let mut flow_nodes: HashMap<String, FlowNode> = nodes
        .into_iter()
        .map(|(k, v)| (k, v.node))
        .collect();

    for node_id in flow_nodes.keys() {
        outgoing.entry(node_id.clone()).or_default();
    }

    flow_nodes = infer_decision_nodes(flow_nodes, &outgoing);
    let (begin_id, end_id) = validate_flow(&flow_nodes, &outgoing)?;

    Ok(Flow {
        nodes: flow_nodes,
        outgoing,
        begin_id,
        end_id,
    })
}

fn is_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.starts_with("flowchart") || lower.starts_with("graph")
}

fn strip_comment(line: &str) -> String {
    if let Some(idx) = line.find("%%") {
        line[..idx].to_string()
    } else {
        line.to_string()
    }
}

fn is_style_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    if lower == "end" {
        return true;
    }
    lower.starts_with("classdef ")
        || lower.starts_with("class ")
        || lower.starts_with("style ")
        || lower.starts_with("linkstyle ")
        || lower.starts_with("click ")
        || lower.starts_with("subgraph ")
        || lower.starts_with("direction ")
}

fn strip_style_tokens(line: &str) -> String {
    let re = regex::Regex::new(r":::[A-Za-z0-9_-]+").unwrap();
    re.replace_all(line, "").to_string()
}

fn try_parse_edge_line(line: &str, line_no: usize) -> Option<(NodeSpec, Option<String>, NodeSpec)> {
    let src_spec = match parse_node_token(line, 0, line_no) {
        Ok((spec, _)) => spec,
        Err(_) => return None,
    };

    let (normalized, label) = normalize_edge_line(line);
    let idx = skip_ws(&normalized, src_spec.node_id.len());
    let after_src = skip_ws(&normalized, idx);
    if !normalized[after_src..].contains('>') {
        if !normalized[after_src..].contains("---") {
            return None;
        }
        let normalized = normalized[..after_src].to_string()
            + &normalized[after_src..].replacen("---", "-->", 1);
        return try_parse_edge_line(&normalized, line_no);
    }

    let re = regex::Regex::new(r"[-.=]+>").unwrap();
    let normalized = re.replace_all(&normalized, "-->").to_string();
    let arrow_idx = normalized.rfind('>')?;
    let dst_start = skip_ws(&normalized, arrow_idx + 1);
    let dst_spec = match parse_node_token(&normalized, dst_start, line_no) {
        Ok((spec, _)) => spec,
        Err(_) => return None,
    };

    Some((src_spec, label, dst_spec))
}

fn parse_node_token(line: &str, idx: usize, line_no: usize) -> Result<(NodeSpec, usize), FlowError> {
    let re = regex::Regex::new(r"^[A-Za-z0-9_][A-Za-z0-9_-]*").unwrap();
    let rest = &line[idx..];
    let m = re.find(rest).ok_or_else(|| {
        FlowError::new(line_error(line_no, "Expected node id"))
    })?;
    let node_id = m.as_str().to_string();
    let mut idx = idx + m.end();

    let shapes: std::collections::HashMap<char, char> =
        [('[', ']'), ('(', ')'), ('{', '}')]
            .into_iter()
            .collect();

    if idx >= line.len() || !shapes.keys().any(|&v| line.chars().nth(idx) == Some(v)) {
        return Ok((NodeSpec { node_id, label: None }, idx));
    }

    let open_char = line.chars().nth(idx).unwrap();
    let close_char = *shapes.get(&open_char).unwrap();
    idx += 1;
    let (label, new_idx) = parse_label(line, idx, close_char, line_no)?;
    Ok((NodeSpec { node_id, label: Some(label) }, new_idx))
}

fn parse_label(line: &str, idx: usize, close_char: char, line_no: usize) -> Result<(String, usize), FlowError> {
    if idx >= line.len() {
        return Err(FlowError::new(line_error(line_no, "Expected node label")));
    }

    if close_char == ')' && line.chars().nth(idx) == Some('[') {
        let (label, mut idx) = parse_label(line, idx + 1, ']', line_no)?;
        idx = skip_ws(line, idx);
        if idx >= line.len() || line.chars().nth(idx) != Some(')') {
            return Err(FlowError::new(line_error(line_no, "Unclosed node label")));
        }
        return Ok((label, idx + 1));
    }

    if line.chars().nth(idx) == Some('"') {
        let mut idx = idx + 1;
        let mut buf = String::new();
        while idx < line.len() {
            let ch = line.chars().nth(idx).unwrap();
            if ch == '"' {
                idx += 1;
                idx = skip_ws(line, idx);
                if idx >= line.len() || line.chars().nth(idx) != Some(close_char) {
                    return Err(FlowError::new(line_error(line_no, "Unclosed node label")));
                }
                return Ok((buf, idx + 1));
            }
            if ch == '\\' && idx + 1 < line.len() {
                buf.push(line.chars().nth(idx + 1).unwrap());
                idx += 2;
                continue;
            }
            buf.push(ch);
            idx += 1;
        }
        return Err(FlowError::new(line_error(line_no, "Unclosed quoted label")));
    }

    let end = line[idx..].find(close_char).ok_or_else(|| {
        FlowError::new(line_error(line_no, "Unclosed node label"))
    })?;
    let label = line[idx..idx + end].trim().to_string();
    if label.is_empty() {
        return Err(FlowError::new(line_error(line_no, "Node label cannot be empty")));
    }
    Ok((label, idx + end + 1))
}

fn skip_ws(line: &str, idx: usize) -> usize {
    let mut idx = idx;
    while idx < line.len() && line.chars().nth(idx).unwrap().is_whitespace() {
        idx += 1;
    }
    idx
}

fn add_node(nodes: &mut HashMap<String, NodeDef>, spec: &NodeSpec, line_no: usize) -> Result<FlowNode, FlowError> {
    let label = spec.label.clone().unwrap_or_else(|| spec.node_id.clone());
    let label_norm = label.trim().to_lowercase();
    if label_norm.is_empty() {
        return Err(FlowError::new(line_error(line_no, "Node label cannot be empty")));
    }

    let kind = if label_norm == "begin" {
        FlowNodeKind::Begin
    } else if label_norm == "end" {
        FlowNodeKind::End
    } else {
        FlowNodeKind::Task
    };

    let node = FlowNode {
        id: spec.node_id.clone(),
        label,
        kind,
    };
    let explicit = spec.label.is_some();

    if let Some(existing) = nodes.get(&spec.node_id) {
        if existing.node == node {
            return Ok(existing.node.clone());
        }
        if !explicit && existing.explicit {
            return Ok(existing.node.clone());
        }
        if explicit && !existing.explicit {
            nodes.insert(spec.node_id.clone(), NodeDef { node: node.clone(), explicit: true });
            return Ok(node);
        }
        return Err(FlowError::new(line_error(
            line_no,
            &format!("Conflicting definition for node \"{}\"", spec.node_id),
        )));
    }

    nodes.insert(spec.node_id.clone(), NodeDef { node: node.clone(), explicit });
    Ok(node)
}

fn try_parse_node_line(line: &str, line_no: usize) -> Option<NodeSpec> {
    match parse_node_token(line, 0, line_no) {
        Ok((spec, _)) => Some(spec),
        Err(_) => None,
    }
}

fn normalize_edge_line(line: &str) -> (String, Option<String>) {
    let mut label = None;
    let mut normalized = line.to_string();

    let pipe_re = regex::Regex::new(r"\|([^|]*)\|").unwrap();
    if let Some(m) = pipe_re.find(&normalized) {
        let matched = m.as_str();
        label = Some(matched[1..matched.len() - 1].trim().to_string());
        if label.as_ref().unwrap().is_empty() {
            label = None;
        }
        normalized = normalized[..m.start()].to_string() + &normalized[m.end()..];
    }

    if label.is_none() {
        let edge_re = regex::Regex::new(r"--\s*([^>-][^>]*)\s*-->").unwrap();
        if let Some(m) = edge_re.find(&normalized) {
            label = Some(m.as_str()[2..m.len() - 3].trim().to_string());
            if label.as_ref().unwrap().is_empty() {
                label = None;
            }
            normalized = normalized[..m.start()].to_string()
                + "-->"
                + &normalized[m.end()..];
        }
    }

    (normalized, label)
}

fn infer_decision_nodes(
    nodes: HashMap<String, FlowNode>,
    outgoing: &HashMap<String, Vec<FlowEdge>>,
) -> HashMap<String, FlowNode> {
    let mut updated = HashMap::new();
    for (node_id, node) in nodes {
        let mut kind = node.kind.clone();
        if matches!(kind, FlowNodeKind::Task) && outgoing.get(&node_id).map(|v| v.len()).unwrap_or(0) > 1 {
            kind = FlowNodeKind::Decision;
        }
        if kind != node.kind {
            updated.insert(
                node_id,
                FlowNode {
                    id: node.id,
                    label: node.label,
                    kind,
                },
            );
        } else {
            updated.insert(node_id, node);
        }
    }
    updated
}

fn line_error(line_no: usize, message: &str) -> String {
    format!("Line {line_no}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_mermaid() {
        let text = r#"
            flowchart TD
            A[begin] --> B[Do something]
            B --> C[end]
        "#;
        let flow = parse_mermaid_flowchart(text).expect("should parse");
        assert_eq!(flow.nodes.len(), 3);
        assert!(flow.begin_id.is_some());
        assert!(flow.end_id.is_some());
    }

    #[test]
    fn debug_mermaid_node() {
        let spec = parse_node_token("A[begin]", 0, 1).unwrap().0;
        assert_eq!(spec.node_id, "A");
        assert_eq!(spec.label, Some("begin".to_string()));
    }

    #[test]
    fn parse_mermaid_decision() {
        let text = r#"
            flowchart TD
            A[begin] --> B{Check}
            B -->|Yes| C[Path A]
            B -->|No| D[Path B]
            C --> E[end]
            D --> E[end]
        "#;
        let flow = parse_mermaid_flowchart(text).expect("should parse");
        let check = flow.nodes.get("B").unwrap();
        assert!(matches!(check.kind, FlowNodeKind::Decision));
    }
}
