use async_trait::async_trait;

/// Asks the user a question.
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

    fn parameters_schema(&self) -> serde_json::Value {
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
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if runtime.approval.yolo {
            return crate::soul::message::ToolReturnValue::Ok {
                output: serde_json::json!({
                    "answers": {},
                    "note": "Running in non-interactive (yolo) mode. Make your own decision."
                })
                .to_string(),
                message: Some("Non-interactive mode, auto-dismissed.".into()),
            };
        }

        let questions = match arguments.get("questions").and_then(|v| v.as_array()) {
            Some(q) => q,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Missing 'questions' array".into(),
                };
            }
        };

        let mut answers = Vec::new();
        for q in questions {
            let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("Question");
            let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("");
            let mut options = q.get("options").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let multi_select = q.get("multi_select").and_then(|v| v.as_bool()).unwrap_or(false);

            // Auto-append "Other" option if not present.
            let has_other = options.iter().any(|opt| {
                opt.get("label")
                    .and_then(|v| v.as_str())
                    .map(|l| l.eq_ignore_ascii_case("other"))
                    .unwrap_or(false)
            });
            if !has_other {
                options.push(serde_json::json!({
                    "label": "Other",
                    "description": "Provide a custom answer."
                }));
            }

            eprintln!("\n [{}] {}", header, question);
            for (i, opt) in options.iter().enumerate() {
                let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                eprintln!("   {}. {} - {}", i + 1, label, desc);
            }
            if multi_select {
                eprint!("  Select one or more (comma-separated): ");
            } else {
                eprint!("  Select one: ");
            }

            use std::io::Write;
            let _ = std::io::stderr().flush();
            let mut input = String::new();
            if let Err(e) = std::io::stdin().read_line(&mut input) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to read input: {e}"),
                };
            }
            let trimmed = input.trim();
            if trimmed.is_empty() {
                answers.push(serde_json::json!({
                    "question": question,
                    "answer": null,
                    "note": "User dismissed the question without answering.",
                }));
            } else {
                answers.push(serde_json::json!({
                    "question": question,
                    "answer": trimmed,
                }));
            }
        }

        crate::soul::message::ToolReturnValue::Ok {
            output: serde_json::to_string_pretty(&answers).unwrap_or_else(|_| "[]".into()),
            message: None,
        }
    }
}
