use async_trait::async_trait;
use serde_json::Value;

/// Enters plan mode.
#[derive(Debug, Clone, Default)]
pub struct EnterPlanMode;

#[async_trait]
impl crate::soul::toolset::Tool for EnterPlanMode {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "Enter plan mode (research and planning only)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    }

    async fn call(
        &self,
        _arguments: Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let session_dir = runtime.session.dir();
        let mut state = crate::session_state::load_session_state(&session_dir);

        if state.plan_mode {
            return crate::soul::message::ToolReturnValue::Error {
                error: "Already in plan mode. Use ExitPlanMode when your plan is ready.".into(),
            };
        }

        if runtime.approval.is_yolo().await {
            state.plan_mode = true;
            if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to save session state: {e}"),
                };
            }
            let plan_path = runtime.session.state.plan_session_id.as_ref().map(|id| {
                crate::share::get_share_dir()
                    .unwrap_or_else(|_| std::env::temp_dir())
                    .join("plans")
                    .join(format!("{id}.md"))
            });
            let plan_path_str = plan_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            return crate::soul::message::ToolReturnValue::Ok {
                output: format!(
                    "Plan mode activated (auto-approved in non-interactive mode).\n\
                     Plan file: {plan_path_str}\n\
                     Workflow: identify key questions about the codebase → \
                     use Agent(subagent_type='explore') to investigate if needed → \
                     design approach → \
                     modify the plan file with WriteFile or StrReplaceFile \
                     (create it with WriteFile first if it does not exist) → \
                     call ExitPlanMode.\n"
                ),
                message: Some("Plan mode on (auto)".into()),
            };
        }

        let hub = match runtime.root_wire_hub.as_ref() {
            Some(h) => h,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Cannot request user confirmation: Wire is not available.".into(),
                };
            }
        };

        let _tool_call = match crate::soul::toolset::get_current_tool_call_or_none() {
            Some(tc) => tc,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "EnterPlanMode must be called from a tool call context.".into(),
                };
            }
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let request = crate::wire::types::WireMessage::QuestionRequest {
            id: request_id.clone(),
            items: vec![crate::wire::types::QuestionItem {
                id: "q0".into(),
                header: "Plan Mode".into(),
                question: "Enter plan mode?".into(),
                options: vec![
                    crate::wire::types::QuestionOption {
                        label: "Yes".into(),
                        description: "Enter plan mode to explore and design an approach".into(),
                    },
                    crate::wire::types::QuestionOption {
                        label: "No".into(),
                        description: "Skip planning, start implementing now".into(),
                    },
                ],
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
                output: "User dismissed without choosing. Proceed with implementation directly.".into(),
                message: Some("Dismissed".into()),
            };
        }

        let chose_yes = answers.values().any(|v| v == "Yes");
        if chose_yes {
            state.plan_mode = true;
            if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to save session state: {e}"),
                };
            }
            let plan_path = runtime.session.state.plan_session_id.as_ref().map(|id| {
                crate::share::get_share_dir()
                    .unwrap_or_else(|_| std::env::temp_dir())
                    .join("plans")
                    .join(format!("{id}.md"))
            });
            let plan_path_str = plan_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            return crate::soul::message::ToolReturnValue::Ok {
                output: format!(
                    "Plan mode activated. You MUST NOT edit code files — only read and plan.\n\
                     Plan file: {plan_path_str}\n\
                     Workflow: identify key questions about the codebase → \
                     use Agent(subagent_type='explore') to investigate if needed → \
                     design approach → \
                     modify the plan file with WriteFile or StrReplaceFile \
                     (create it with WriteFile first if it does not exist) → \
                     call ExitPlanMode.\n\
                     Use AskUserQuestion only to clarify missing requirements or choose \
                     between approaches.\n\
                     Do NOT use AskUserQuestion to ask about plan approval.\n"
                ),
                message: Some("Plan mode on".into()),
            };
        } else {
            return crate::soul::message::ToolReturnValue::Ok {
                output: (
                    "User declined to enter plan mode. Please check with user whether \
                     to proceed with implementation directly."
                )
                .into(),
                message: Some("Declined".into()),
            };
        }
    }
}
