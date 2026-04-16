use async_trait::async_trait;

/// Exits plan mode.
#[derive(Debug, Clone, Default)]
pub struct ExitPlanMode;

#[async_trait]
impl crate::soul::toolset::Tool for ExitPlanMode {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and resume normal tool access."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    }

    async fn call(
        &self,
        _arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let session_dir = runtime.session.dir();
        let mut state = crate::session_state::load_session_state(&session_dir);
        state.plan_mode = false;
        state.plan_session_id = None;
        state.plan_slug = None;
        if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to save session state: {e}"),
            };
        }
        crate::soul::message::ToolReturnValue::Ok {
            output: "Exited plan mode.".into(),
            message: None,
        }
    }
}
