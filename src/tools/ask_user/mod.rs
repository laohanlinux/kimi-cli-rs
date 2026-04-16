use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

/// Asks the user a question via the wire protocol.
#[derive(Debug, Clone, Default)]
pub struct AskUserQuestion;

#[async_trait]
impl crate::soul::toolset::Tool for AskUserQuestion {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user one or more questions with multiple-choice answers."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string" },
                            "header": { "type": "string" },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    }
                                }
                            },
                            "multi_select": { "type": "boolean" }
                        },
                        "required": ["question", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    async fn call(
        &self,
        arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if runtime.approval.is_yolo().await {
            return crate::soul::message::ToolReturnValue::Ok {
                output: serde_json::json!({
                    "answers": {},
                    "note": "Running in non-interactive (yolo) mode. Make your own decision."
                })
                .to_string(),
                message: Some("Non-interactive mode, auto-dismissed.".into()),
            };
        }

        let hub = match runtime.root_wire_hub.as_ref() {
            Some(h) => h,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Cannot ask user questions: Wire is not available.".into(),
                };
            }
        };

        let _tool_call = match crate::soul::toolset::get_current_tool_call_or_none() {
            Some(tc) => tc,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "AskUserQuestion must be called from a tool call context.".into(),
                };
            }
        };

        let questions_arr = match arguments.get("questions").and_then(|v| v.as_array()) {
            Some(q) => q,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Missing 'questions' array".into(),
                };
            }
        };

        let mut items = Vec::new();
        for (idx, q) in questions_arr.iter().enumerate() {
            let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let multi_select = q.get("multi_select").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut options = Vec::new();
            if let Some(opts) = q.get("options").and_then(|v| v.as_array()) {
                for opt in opts {
                    let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    options.push(crate::wire::types::QuestionOption { label, description: desc });
                }
            }
            // Auto-append "Other" option if not present.
            let has_other = options.iter().any(|o| o.label.eq_ignore_ascii_case("other"));
            if !has_other {
                options.push(crate::wire::types::QuestionOption {
                    label: "Other".into(),
                    description: "Provide a custom answer.".into(),
                });
            }
            items.push(crate::wire::types::QuestionItem {
                id: format!("q{idx}"),
                header,
                question,
                options,
                multi_select,
            });
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let request = crate::wire::types::WireMessage::QuestionRequest {
            id: request_id.clone(),
            items,
        };

        let mut rx = hub.subscribe();
        hub.publish(request);

        let answers = match wait_for_question_response(&mut rx, &request_id, 300.0).await {
            Ok(a) => a,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to get user response: {e}"),
                };
            }
        };

        if answers.is_empty() {
            return crate::soul::message::ToolReturnValue::Ok {
                output: serde_json::json!({
                    "answers": {},
                    "note": "User dismissed the question without answering."
                })
                .to_string(),
                message: Some("User dismissed the question without answering.".into()),
            };
        }

        let formatted = serde_json::json!({ "answers": answers });
        crate::soul::message::ToolReturnValue::Ok {
            output: formatted.to_string(),
            message: Some("User has answered.".into()),
        }
    }
}

pub async fn wait_for_question_response(
    rx: &mut tokio::sync::broadcast::Receiver<crate::wire::types::WireMessage>,
    request_id: &str,
    timeout_secs: f64,
) -> crate::error::Result<HashMap<String, String>> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs_f64(timeout_secs);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(m)) => m,
            Ok(Err(_)) => return Err(crate::error::KimiCliError::Generic("Wire channel closed".into())),
            Err(_) => return Err(crate::error::KimiCliError::Generic("Question timed out".into())),
        };
        if let crate::wire::types::WireMessage::QuestionResponse { request_id: rid, answers } = msg {
            if rid == request_id {
                return Ok(answers);
            }
        }
    }
}
