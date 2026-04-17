use std::collections::HashMap;

/// Print-mode UI renderer with streaming wire support.
#[derive(Debug, Clone)]
pub struct PrintUi {
    verbose: bool,
}

impl Default for PrintUi {
    fn default() -> Self {
        Self { verbose: false }
    }
}

impl PrintUi {
    /// Creates a new print UI.
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }

    /// Runs a single turn in print mode, streaming wire events to stderr.
    pub async fn run(
        &mut self,
        cli: &mut crate::app::KimiCLI,
        user_input: Vec<crate::soul::message::ContentPart>,
    ) -> crate::error::Result<crate::soul::TurnOutcome> {
        let (wire_tx, mut wire_rx) =
            tokio::sync::mpsc::channel::<crate::wire::types::WireMessage>(256);
        let verbose = self.verbose;

        let print_handle = tokio::spawn(async move {
            while let Some(msg) = wire_rx.recv().await {
                Self::render_message(verbose, &msg);
            }
        });

        let outcome = cli
            .run_with_wire(user_input, move |wire| {
                Box::pin(async move {
                    let mut ui_side = wire.ui_side();
                    while let Some(msg) = ui_side.recv().await {
                        match &msg {
                            crate::wire::types::WireMessage::ApprovalRequest { id, .. } => {
                                let _ = ui_side
                                    .send_response(
                                        crate::wire::types::WireMessage::ApprovalResponse {
                                            request_id: id.clone(),
                                            response: "reject".into(),
                                            feedback: Some(
                                                "Print mode does not support interactive approval. Use --yolo to auto-approve."
                                                    .into(),
                                            ),
                                        },
                                    )
                                    .await;
                            }
                            crate::wire::types::WireMessage::QuestionRequest { id, .. } => {
                                let _ = ui_side
                                    .send_response(
                                        crate::wire::types::WireMessage::QuestionResponse {
                                            request_id: id.clone(),
                                            answers: HashMap::new(),
                                        },
                                    )
                                    .await;
                            }
                            _ => {}
                        }
                        if wire_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                })
            }, None)
            .await;

        let _ = print_handle.await;
        outcome
    }

    fn render_message(verbose: bool, msg: &crate::wire::types::WireMessage) {
        match msg {
            crate::wire::types::WireMessage::StepBegin { step_no } => {
                eprintln!("  [step {step_no}]");
            }
            crate::wire::types::WireMessage::ToolCall { name, .. } => {
                eprintln!("  [tool] {name}");
            }
            crate::wire::types::WireMessage::ToolResult { result, .. } => {
                let text = result.extract_text();
                let preview = if text.len() > 300 {
                    format!("{}...", &text[..300].trim_end())
                } else {
                    text.trim_end().to_string()
                };
                if !preview.is_empty() {
                    eprintln!("  [result] {preview}");
                } else {
                    eprintln!("  [result] (empty)");
                }
            }
            crate::wire::types::WireMessage::ThinkPart { thought } => {
                eprintln!("  [think] {thought}");
            }
            crate::wire::types::WireMessage::Notification { text } => {
                eprintln!("  [notify] {text}");
            }
            crate::wire::types::WireMessage::McpLoadingBegin => {
                eprintln!("  [mcp] loading...");
            }
            crate::wire::types::WireMessage::McpLoadingEnd => {
                eprintln!("  [mcp] ready");
            }
            _ => {}
        }

        if verbose {
            match msg {
                crate::wire::types::WireMessage::TurnBegin { .. } => {
                    eprintln!("  [turn begin]");
                }
                crate::wire::types::WireMessage::TurnEnd { stop_reason } => {
                    eprintln!("  [turn end] {stop_reason}");
                }
                crate::wire::types::WireMessage::StatusUpdate { snapshot } => {
                    eprintln!(
                        "  [status] ctx={:.0}% yolo={} plan={}",
                        snapshot.context_usage * 100.0,
                        snapshot.yolo_enabled,
                        snapshot.plan_mode
                    );
                }
                crate::wire::types::WireMessage::CompactionBegin => {
                    eprintln!("  [compaction begin]");
                }
                crate::wire::types::WireMessage::CompactionEnd => {
                    eprintln!("  [compaction end]");
                }
                crate::wire::types::WireMessage::SubagentEvent { agent_id, event } => {
                    eprintln!("  [subagent {agent_id}] {event}");
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_ui_default() {
        let ui = PrintUi::default();
        assert!(!ui.verbose);
    }

    #[test]
    fn print_ui_new_verbose() {
        let ui = PrintUi::new(true);
        assert!(ui.verbose);
    }

    #[test]
    fn render_message_step_begin() {
        let msg = crate::wire::types::WireMessage::StepBegin { step_no: 1 };
        PrintUi::render_message(false, &msg);
    }

    #[test]
    fn render_message_tool_call() {
        let msg = crate::wire::types::WireMessage::ToolCall {
            tool_call_id: "1".into(),
            name: "Shell".into(),
            arguments: serde_json::json!({"command": "echo hi"}),
        };
        PrintUi::render_message(false, &msg);
    }

    #[test]
    fn render_message_verbose_turn_end() {
        let msg = crate::wire::types::WireMessage::TurnEnd {
            stop_reason: "completed".into(),
        };
        PrintUi::render_message(true, &msg);
    }
}
