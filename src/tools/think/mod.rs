use async_trait::async_trait;

/// Allows the model to emit a thought.
#[derive(Debug, Clone, Default)]
pub struct Think;

#[async_trait]
impl crate::soul::toolset::Tool for Think {
    fn name(&self) -> &str {
        "Think"
    }

    fn description(&self) -> &str {
        "Emit a free-form thought that is not shown to the user."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "thought": { "type": "string", "description": "The thought to record" }
            },
            "required": ["thought"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let thought = arguments
            .get("thought")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        crate::soul::message::ToolReturnValue::Ok {
            output: thought,
            message: None,
        }
    }
}
