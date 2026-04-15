use async_trait::async_trait;

/// Sends a D-Mail to revert context to a previous checkpoint.
#[derive(Debug, Clone, Default)]
pub struct SendDMail;

#[async_trait]
impl crate::soul::toolset::Tool for SendDMail {
    fn name(&self) -> &str {
        "SendDMail"
    }

    fn description(&self) -> &str {
        "Send a D-Mail to revert context to a previous checkpoint."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "checkpoint_id": { "type": "integer", "description": "Checkpoint ID to revert to" },
                "message": { "type": "string", "description": "Message to record" }
            },
            "required": ["checkpoint_id", "message"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let checkpoint_id = arguments
            .get("checkpoint_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let message = arguments
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let dmail = crate::soul::denwa_renji::DMail {
            message,
            checkpoint_id,
        };

        match runtime.denwa_renji.send_dmail(dmail) {
            Ok(()) => crate::soul::message::ToolReturnValue::Ok {
                output: String::new(),
                message: Some(
                    "If you see this message, the D-Mail was NOT sent successfully. \
                     This may be because some other tool that needs approval was rejected."
                        .into(),
                ),
            },
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to send D-Mail: {e}"),
            },
        }
    }
}
