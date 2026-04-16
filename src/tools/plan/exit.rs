use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;

/// Exits plan mode.
#[derive(Debug, Clone, Default)]
pub struct ExitPlanMode;

const RESERVED_LABELS: &[&str] = &["reject", "revise", "approve", "reject and exit"];

#[async_trait]
impl crate::soul::toolset::Tool for ExitPlanMode {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and resume normal tool access."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "options": {
                    "type": "array",
                    "maxLength": 3,
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string" },
                            "description": { "type": "string" }
                        }
                    },
                    "description": "When the plan contains multiple alternative approaches, list them here so the user can choose which one to execute. 2-3 options."
                }
            }
        })
    }

    async fn call(
        &self,
        arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let session_dir = runtime.session.dir();
        let mut state = crate::session_state::load_session_state(&session_dir);

        if !state.plan_mode {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Not in plan mode. ExitPlanMode is only available during plan mode.".into(),
            };
        }

        let plan_path = runtime.session.state.plan_session_id.as_ref().map(|id| {
            crate::share::get_share_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("plans")
                .join(format!("{id}.md"))
        });

        let plan_content = if let Some(ref path) = plan_path {
            match tokio::fs::read_to_string(path).await {
                Ok(text) if !text.trim().is_empty() => text,
                _ => {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!(
                            "No plan file found. Write your plan to {} first, then call ExitPlanMode.",
                            path.display()
                        ),
                    };
                }
            }
        } else {
            return crate::soul::message::ToolReturnValue::Error {
                error: "No plan session ID found. Cannot locate plan file.".into(),
            };
        };

        if runtime.approval.is_yolo().await {
            state.plan_mode = false;
            if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to save session state: {e}"),
                };
            }
            let plan_path_str = plan_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            return crate::soul::message::ToolReturnValue::Ok {
                output: format!(
                    "Plan approved (auto-approved in non-interactive mode). Plan mode deactivated. All tools are now available.\n\
                     Plan saved to: {plan_path_str}\n\n\
                     ## Approved Plan:\n{plan_content}"
                ),
                message: Some("Plan approved (auto)".into()),
            };
        }

        let hub = match runtime.root_wire_hub.as_ref() {
            Some(h) => h,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Cannot present plan: Wire is not available.".into(),
                };
            }
        };

        let _tool_call = match crate::soul::toolset::get_current_tool_call_or_none() {
            Some(tc) => tc,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "ExitPlanMode must be called from a tool call context.".into(),
                };
            }
        };

        let options_arr = arguments.get("options").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let has_options = options_arr.len() >= 2;

        // Validate reserved labels and uniqueness
        let mut labels = HashSet::new();
        for opt in &options_arr {
            if let Some(label) = opt.get("label").and_then(|v| v.as_str()) {
                let lower = label.trim().to_lowercase();
                if RESERVED_LABELS.contains(&lower.as_str()) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Option label '{label}' is reserved."),
                    };
                }
                if !labels.insert(label.to_string()) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: "Option labels must be unique. Found duplicate label(s).".into(),
                    };
                }
            }
        }

        let reject_options = vec![
            crate::wire::types::QuestionOption {
                label: "Reject".into(),
                description: "Reject and stay in plan mode".into(),
            },
            crate::wire::types::QuestionOption {
                label: "Reject and Exit".into(),
                description: "Reject and exit plan mode".into(),
            },
        ];

        let mut question_options: Vec<crate::wire::types::QuestionOption> = if has_options {
            options_arr
                .iter()
                .filter_map(|opt| {
                    let label = opt.get("label").and_then(|v| v.as_str())?.to_string();
                    let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Some(crate::wire::types::QuestionOption { label, description: desc })
                })
                .collect()
        } else {
            vec![crate::wire::types::QuestionOption {
                label: "Approve".into(),
                description: "Exit plan mode and start execution".into(),
            }]
        };
        question_options.extend(reject_options);

        let plan_path_str = plan_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        hub.publish(crate::wire::types::WireMessage::PlanDisplay {
            content: plan_content.clone(),
        });

        let request_id = uuid::Uuid::new_v4().to_string();
        let request = crate::wire::types::WireMessage::QuestionRequest {
            id: request_id.clone(),
            items: vec![crate::wire::types::QuestionItem {
                id: "q0".into(),
                header: "Plan".into(),
                question: "Approve this plan".into(),
                options: question_options,
                multi_select: false,
            }],
        };

        let mut rx = hub.subscribe();
        hub.publish(request);

        let answers = match crate::tools::ask_user::wait_for_question_response(&mut rx, &request_id, 300.0).await {
            Ok(a) => a,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to get user response: {e}"),
                };
            }
        };

        if answers.is_empty() {
            return crate::soul::message::ToolReturnValue::Ok {
                output: (
                    "User dismissed without choosing. Plan mode remains active. \
                     Continue working on your plan or call ExitPlanMode again when ready."
                )
                .into(),
                message: Some("Dismissed".into()),
            };
        }

        let chose_reject_and_exit = answers.values().any(|v| v == "Reject and Exit");
        if chose_reject_and_exit {
            state.plan_mode = false;
            if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to save session state: {e}"),
                };
            }
            return crate::soul::message::ToolReturnValue::Error {
                error: (
                    "Plan rejected by user. Plan mode deactivated. All tools are now available. \
                     Wait for the user's next message."
                )
                .into(),
            };
        }

        let chose_reject = answers.values().any(|v| v == "Reject");
        if chose_reject {
            return crate::soul::message::ToolReturnValue::Error {
                error: (
                    "Plan rejected by user. Stay in plan mode. \
                     The user will provide feedback via conversation. \
                     Wait for the user's next message before revising."
                )
                .into(),
            };
        }

        if has_options {
            let option_labels: HashSet<String> = options_arr
                .iter()
                .filter_map(|opt| opt.get("label").and_then(|v| v.as_str()).map(String::from))
                .collect();
            let chosen_option = answers.values().find(|v| option_labels.contains(*v)).cloned();
            if let Some(chosen) = chosen_option {
                state.plan_mode = false;
                if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to save session state: {e}"),
                    };
                }
                return crate::soul::message::ToolReturnValue::Ok {
                    output: format!(
                        "Plan approved by user. Selected approach: \"{chosen}\"\n\
                         Plan mode deactivated. All tools are now available.\n\
                         Plan saved to: {plan_path_str}\n\n\
                         IMPORTANT: Execute ONLY the selected approach \"{chosen}\". \
                         Ignore other approaches in the plan.\n\n\
                         ## Approved Plan:\n{plan_content}"
                    ),
                    message: Some(format!("Plan approved: {chosen}")),
                };
            }
        } else {
            let chose_approve = answers.values().any(|v| v == "Approve");
            if chose_approve {
                state.plan_mode = false;
                if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to save session state: {e}"),
                    };
                }
                return crate::soul::message::ToolReturnValue::Ok {
                    output: format!(
                        "Plan approved by user. Plan mode deactivated. All tools are now available.\n\
                         Plan saved to: {plan_path_str}\n\n\
                         ## Approved Plan:\n{plan_content}"
                    ),
                    message: Some("Plan approved".into()),
                };
            }
        }

        // Revise fallback
        let feedback = answers
            .values()
            .find(|v| !["Approve", "Reject", "Reject and Exit"].contains(&v.as_str()))
            .cloned()
            .unwrap_or_default();
        let msg = if feedback.is_empty() {
            (
                "User wants to revise the plan. Stay in plan mode. \
                 Wait for the user's next message with feedback before revising."
            )
            .into()
        } else {
            format!(
                "User wants to revise the plan. Stay in plan mode. \
                 Revise based on the feedback below.\n\n\
                 User feedback: {feedback}"
            )
        };
        crate::soul::message::ToolReturnValue::Ok {
            output: msg,
            message: Some("Plan revision requested".into()),
        }
    }
}
