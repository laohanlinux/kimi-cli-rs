use async_trait::async_trait;

/// Sends a D-Mail (time-travel message to a past session).
#[derive(Debug, Clone, Default)]
pub struct SendDMail;

#[async_trait]
impl crate::soul::toolset::Tool for SendDMail {
    fn name(&self) -> &str {
        "SendDMail"
    }

    fn description(&self) -> &str {
        "Send a message to a past session (D-Mail)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "target_session_id": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["target_session_id", "message"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let target_id = arguments
            .get("target_session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let message = arguments
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if target_id.is_empty() || message.is_empty() {
            return crate::soul::message::ToolReturnValue::Error {
                error: "target_session_id and message are required".into(),
            };
        }

        let metadata = crate::metadata::load_metadata();
        let mut found = None;
        for wd in &metadata.work_dirs {
            let sessions_dir = wd.sessions_dir();
            let candidate = sessions_dir.join(target_id);
            if candidate.is_dir() {
                found = Some(candidate);
                break;
            }
        }

        let Some(target_dir) = found else {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Target session {target_id} not found"),
            };
        };

        let dmail_path = target_dir.join("dmail.jsonl");
        let record = serde_json::json!({
            "from_session_id": runtime.session.id,
            "message": message,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        });
        let line = format!("{}\n", serde_json::to_string(&record).unwrap_or_default());
        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&dmail_path)
        {
            Ok(f) => f,
            Err(e) => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: format!("Failed to open dmail file: {e}"),
                };
            }
        };
        use std::io::Write;
        if let Err(e) = file.write_all(line.as_bytes()) {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to write dmail: {e}"),
            };
        }

        crate::soul::message::ToolReturnValue::Ok {
            output: format!("D-Mail sent to session {target_id}"),
            message: None,
        }
    }
}
