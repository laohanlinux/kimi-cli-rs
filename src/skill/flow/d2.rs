use crate::skill::flow::{Flow, FlowEdge, FlowError, FlowNode, FlowNodeKind, validate_flow};
use std::collections::HashMap;

const NODE_ID_PATTERN: &str = r"[A-Za-z0-9_][A-Za-z0-9_./-]*";

/// Internal node definition with explicit flag.
#[derive(Debug, Clone)]
struct NodeDef {
    node: FlowNode,
    explicit: bool,
}

/// Parses a D2 flowchart into a Flow.
#[tracing::instrument(level = "debug")]
pub fn parse_d2_flowchart(text: &str) -> Result<Flow, FlowError> {
    let text = normalize_markdown_blocks(text)?;
    let mut nodes: HashMap<String, NodeDef> = HashMap::new();
    let mut outgoing: HashMap<String, Vec<FlowEdge>> = HashMap::new();

    for (line_no, statement) in iter_top_level_statements(&text)? {
        if has_unquoted_token(&statement, "->") {
            parse_edge_statement(&statement, line_no, &mut nodes, &mut outgoing)?;
        } else {
            parse_node_statement(&statement, line_no, &mut nodes)?;
        }
    }

    let mut flow_nodes: HashMap<String, FlowNode> =
        nodes.into_iter().map(|(k, v)| (k, v.node)).collect();

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

fn normalize_markdown_blocks(text: &str) -> Result<String, FlowError> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut out_lines: Vec<String> = Vec::new();
    let mut i = 0;
    let mut line_no = 1usize;

    while i < lines.len() {
        let line = lines[i];
        let (prefix, suffix) = split_unquoted_once(line, ":");
        let suffix = match suffix {
            Some(s) => s,
            None => {
                out_lines.push(line.to_string());
                i += 1;
                line_no += 1;
                continue;
            }
        };

        let suffix_stripped = strip_unquoted_comment(&suffix);
        let suffix_clean = suffix_stripped.trim();
        if !suffix_clean.eq("|md") {
            out_lines.push(line.to_string());
            i += 1;
            line_no += 1;
            continue;
        }

        let start_line = line_no;
        let mut block_lines: Vec<&str> = Vec::new();
        i += 1;
        line_no += 1;
        while i < lines.len() {
            let block_line = lines[i];
            if block_line.trim() == "|" {
                break;
            }
            block_lines.push(block_line);
            i += 1;
            line_no += 1;
        }
        if i >= lines.len() {
            return Err(FlowError::new(line_error(
                start_line,
                "Unclosed markdown block",
            )));
        }

        let dedented = dedent_block(&block_lines);
        if !dedented.is_empty() {
            let escaped: Vec<String> = dedented.iter().map(|l| escape_quoted_line(l)).collect();
            out_lines.push(format!("{}: \"{}\"", prefix, escaped.join("\\n")));
        } else {
            out_lines.push(format!("{}: \"\"", prefix));
        }

        i += 1;
        line_no += 1;
    }

    Ok(out_lines.join("\n"))
}

fn strip_unquoted_comment(text: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for (bidx, ch) in text.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            escape = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if ch == '#' && !in_single && !in_double {
            return text[..bidx].to_string();
        }
    }
    text.to_string()
}

fn dedent_block(lines: &[&str]) -> Vec<String> {
    let mut indent: Option<usize> = None;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let stripped = line.trim_start_matches(|c| c == ' ' || c == '\t');
        let lead = line.len() - stripped.len();
        if indent.is_none() || lead < indent.unwrap() {
            indent = Some(lead);
        }
    }
    let indent = indent.unwrap_or(0);
    lines
        .iter()
        .map(|line| {
            if line.len() >= indent {
                line[indent..].to_string()
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn escape_quoted_line(line: &str) -> String {
    line.replace("\\", "\\\\").replace('"', "\\\"")
}

fn iter_top_level_statements(text: &str) -> Result<Vec<(usize, String)>, FlowError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut brace_depth = 0isize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut drop_line = false;
    let mut buf = String::new();
    let mut line_no = 1usize;
    let mut stmt_line = 1usize;
    let mut i = 0usize;
    let mut result = Vec::new();

    while i < text.len() {
        let ch = text.chars().nth(i).unwrap();
        let next_ch = text.chars().nth(i + 1).unwrap_or('\0');

        if ch == '\\' && next_ch == '\n' {
            i += 2;
            line_no += 1;
            continue;
        }

        if ch == '\n' {
            if (in_single || in_double) && brace_depth == 0 && !drop_line {
                buf.push('\n');
                line_no += 1;
                i += 1;
                continue;
            }
            if brace_depth == 0 && !in_single && !in_double && !drop_line {
                let statement = buf.trim().to_string();
                if !statement.is_empty() {
                    result.push((stmt_line, statement));
                }
            }
            buf.clear();
            drop_line = false;
            stmt_line = line_no + 1;
            line_no += 1;
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            if ch == '#' {
                while i < text.len() && text.chars().nth(i).unwrap() != '\n' {
                    i += 1;
                }
                continue;
            }
            if ch == '{' {
                if brace_depth == 0 {
                    let statement = buf.trim().to_string();
                    if !statement.is_empty() {
                        result.push((stmt_line, statement));
                    }
                    drop_line = true;
                    buf.clear();
                }
                brace_depth += 1;
                i += 1;
                continue;
            }
            if ch == '}' && brace_depth > 0 {
                brace_depth -= 1;
                i += 1;
                continue;
            }
            if ch == '}' && brace_depth == 0 {
                return Err(FlowError::new(line_error(line_no, "Unmatched '}'")));
            }
        }

        if ch == '\'' && !in_double && !escape {
            in_single = !in_single;
        } else if ch == '"' && !in_single && !escape {
            in_double = !in_double;
        }

        if escape {
            escape = false;
        } else if ch == '\\' && (in_single || in_double) {
            escape = true;
        }

        if brace_depth == 0 && !drop_line {
            buf.push(ch);
        }

        i += 1;
    }

    if brace_depth != 0 {
        return Err(FlowError::new(line_error(line_no, "Unclosed '{' block")));
    }
    if in_single || in_double {
        return Err(FlowError::new(line_error(line_no, "Unclosed string")));
    }

    let statement = buf.trim().to_string();
    if !statement.is_empty() {
        result.push((stmt_line, statement));
    }

    Ok(result)
}

fn has_unquoted_token(text: &str, token: &str) -> bool {
    split_on_token(text, token).len() > 1
}

fn split_on_token(text: &str, token: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut i = 0;

    while i < text.len() {
        if !in_single && !in_double && text[i..].starts_with(token) {
            parts.push(buf.trim().to_string());
            buf.clear();
            i += token.len();
            continue;
        }
        let ch = text.chars().nth(i).unwrap();
        if escape {
            escape = false;
        } else if ch == '\\' && (in_single || in_double) {
            escape = true;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        }
        buf.push(ch);
        i += 1;
    }

    parts.push(buf.trim().to_string());
    parts
}

fn parse_edge_statement(
    statement: &str,
    line_no: usize,
    nodes: &mut HashMap<String, NodeDef>,
    outgoing: &mut HashMap<String, Vec<FlowEdge>>,
) -> Result<(), FlowError> {
    let mut parts = split_on_token(statement, "->");
    if parts.len() < 2 {
        return Err(FlowError::new(line_error(line_no, "Expected edge arrow")));
    }

    let last_part = parts.pop().unwrap();
    let (target_text, edge_label) = split_unquoted_once(&last_part, ":");
    parts.push(target_text.to_string());

    let mut node_ids = Vec::new();
    for (idx, part) in parts.iter().enumerate() {
        let node_id = parse_node_id(part, line_no, idx < parts.len() - 1)?;
        node_ids.push(node_id);
    }

    if node_ids.iter().any(|id| is_property_path(id)) {
        return Ok(());
    }
    if node_ids.len() < 2 {
        return Err(FlowError::new(line_error(
            line_no,
            "Edge must have at least two nodes",
        )));
    }

    let label = edge_label
        .as_ref()
        .map(|l| parse_label(l, line_no))
        .transpose()?;
    for idx in 0..node_ids.len() - 1 {
        let edge_label = if idx == node_ids.len() - 2 {
            label.clone()
        } else {
            None
        };
        let edge = FlowEdge {
            src: node_ids[idx].clone(),
            dst: node_ids[idx + 1].clone(),
            label: edge_label,
        };
        outgoing.entry(edge.src.clone()).or_default().push(edge);
        outgoing.entry(node_ids[idx + 1].clone()).or_default();
    }

    for node_id in &node_ids {
        add_node(nodes, node_id, None, false, line_no)?;
    }

    Ok(())
}

fn parse_node_statement(
    statement: &str,
    line_no: usize,
    nodes: &mut HashMap<String, NodeDef>,
) -> Result<(), FlowError> {
    let (node_text, label_text) = split_unquoted_once(statement, ":");
    if label_text.is_some() && is_property_path(&node_text) {
        return Ok(());
    }
    let node_id = parse_node_id(&node_text, line_no, false)?;
    let mut label = None;
    let mut explicit = false;
    if let Some(ref lt) = label_text {
        if lt.trim().is_empty() {
            return Ok(());
        }
        label = Some(parse_label(lt, line_no)?);
        explicit = true;
    }
    add_node(nodes, &node_id, label, explicit, line_no)?;
    Ok(())
}

fn parse_node_id(
    text: &str,
    line_no: usize,
    allow_inline_label: bool,
) -> Result<String, FlowError> {
    let cleaned = text.trim();
    let cleaned = if allow_inline_label {
        split_unquoted_once(cleaned, ":").0.to_string()
    } else {
        cleaned.to_string()
    };
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Err(FlowError::new(line_error(line_no, "Expected node id")));
    }
    let re = regex::Regex::new(&format!("^{}$", NODE_ID_PATTERN)).unwrap();
    if !re.is_match(cleaned) {
        return Err(FlowError::new(line_error(
            line_no,
            &format!("Invalid node id \"{cleaned}\""),
        )));
    }
    Ok(cleaned.to_string())
}

fn is_property_path(node_id: &str) -> bool {
    if !node_id.contains('.') {
        return false;
    }
    let parts: Vec<&str> = node_id.split('.').filter(|p| !p.is_empty()).collect();
    let property_segments = [
        "shape",
        "style",
        "label",
        "link",
        "icon",
        "near",
        "width",
        "height",
        "direction",
        "grid-rows",
        "grid-columns",
        "grid-gap",
        "font-size",
        "font-family",
        "font-color",
        "stroke",
        "fill",
        "opacity",
        "padding",
        "border-radius",
        "shadow",
        "sketch",
        "animated",
        "multiple",
        "constraint",
        "tooltip",
    ];
    for part in &parts[1..] {
        if property_segments.contains(part) || part.starts_with("style") {
            return true;
        }
    }
    parts
        .last()
        .map(|p| property_segments.contains(p))
        .unwrap_or(false)
}

fn parse_label(text: &str, line_no: usize) -> Result<String, FlowError> {
    let label = text.trim();
    if label.is_empty() {
        return Err(FlowError::new(line_error(line_no, "Label cannot be empty")));
    }
    if label.starts_with('\'') || label.starts_with('"') {
        parse_quoted_label(label, line_no)
    } else {
        Ok(label.to_string())
    }
}

fn parse_quoted_label(text: &str, line_no: usize) -> Result<String, FlowError> {
    let quote = text.chars().next().unwrap();
    let mut buf = String::new();
    let mut escape = false;
    let mut i = 1;
    while i < text.len() {
        let ch = text.chars().nth(i).unwrap();
        if escape {
            buf.push(ch);
            escape = false;
            i += 1;
            continue;
        }
        if ch == '\\' {
            escape = true;
            i += 1;
            continue;
        }
        if ch == quote {
            let trailing = text[i + 1..].trim();
            if !trailing.is_empty() {
                return Err(FlowError::new(line_error(
                    line_no,
                    "Unexpected trailing content",
                )));
            }
            return Ok(buf);
        }
        buf.push(ch);
        i += 1;
    }
    Err(FlowError::new(line_error(line_no, "Unclosed quoted label")))
}

fn split_unquoted_once(text: &str, token: &str) -> (String, Option<String>) {
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for (bidx, ch) in text.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && (in_single || in_double) {
            escape = true;
            continue;
        }
        if ch == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if !in_single && !in_double && text[bidx..].starts_with(token) {
            let end = bidx + token.len();
            return (
                text[..bidx].trim().to_string(),
                Some(text[end..].trim().to_string()),
            );
        }
    }
    (text.trim().to_string(), None)
}

fn add_node(
    nodes: &mut HashMap<String, NodeDef>,
    node_id: &str,
    label: Option<String>,
    explicit: bool,
    line_no: usize,
) -> Result<FlowNode, FlowError> {
    let label = label.unwrap_or_else(|| node_id.to_string());
    let label_norm = label.trim().to_lowercase();
    if label_norm.is_empty() {
        return Err(FlowError::new(line_error(
            line_no,
            "Node label cannot be empty",
        )));
    }

    let kind = if label_norm == "begin" {
        FlowNodeKind::Begin
    } else if label_norm == "end" {
        FlowNodeKind::End
    } else {
        FlowNodeKind::Task
    };

    let node = FlowNode {
        id: node_id.to_string(),
        label,
        kind,
    };

    if let Some(existing) = nodes.get(node_id) {
        if existing.node == node {
            return Ok(existing.node.clone());
        }
        if !explicit && existing.explicit {
            return Ok(existing.node.clone());
        }
        if explicit && !existing.explicit {
            nodes.insert(
                node_id.to_string(),
                NodeDef {
                    node: node.clone(),
                    explicit: true,
                },
            );
            return Ok(node);
        }
        return Err(FlowError::new(line_error(
            line_no,
            &format!("Conflicting definition for node \"{node_id}\""),
        )));
    }

    nodes.insert(
        node_id.to_string(),
        NodeDef {
            node: node.clone(),
            explicit,
        },
    );
    Ok(node)
}

fn infer_decision_nodes(
    nodes: HashMap<String, FlowNode>,
    outgoing: &HashMap<String, Vec<FlowEdge>>,
) -> HashMap<String, FlowNode> {
    let mut updated = HashMap::new();
    for (node_id, node) in nodes {
        let mut kind = node.kind.clone();
        if matches!(kind, FlowNodeKind::Task)
            && outgoing.get(&node_id).map(|v| v.len()).unwrap_or(0) > 1
        {
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
    fn parse_simple_d2() {
        let text = r#"
            begin: "begin"
            task1: "Do something"
            end: "end"
            begin -> task1 -> end
        "#;
        let flow = parse_d2_flowchart(text).expect("should parse");
        assert_eq!(flow.nodes.len(), 3);
        assert!(flow.begin_id.is_some());
        assert!(flow.end_id.is_some());
    }

    #[test]
    fn parse_d2_decision() {
        let text = r#"
            begin -> check
            check -> path_a
            check -> path_b
            path_a -> end
            path_b -> end
        "#;
        let flow = parse_d2_flowchart(text).expect("should parse");
        let check = flow.nodes.get("check").unwrap();
        assert!(matches!(check.kind, FlowNodeKind::Decision));
    }
}
