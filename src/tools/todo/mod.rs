use async_trait::async_trait;

/// Sets the todo list for the session.
#[derive(Debug, Clone, Default)]
pub struct SetTodoList;

#[async_trait]
impl crate::soul::toolset::Tool for SetTodoList {
    fn name(&self) -> &str {
        "SetTodoList"
    }

    fn description(&self) -> &str {
        "Set or update the session todo list."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "content": { "type": "string" },
                            "done": { "type": "boolean" }
                        },
                        "required": ["id", "content", "done"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let todos = match arguments.get("todos").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => {
                return crate::soul::message::ToolReturnValue::Error {
                    error: "Missing 'todos' array".into(),
                };
            }
        };

        let mut items = Vec::new();
        for t in todos {
            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let done = t.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
            items.push(crate::session_state::TodoItemState { id, content, done });
        }

        let session_dir = runtime.session.dir();
        let mut state = crate::session_state::load_session_state(&session_dir);
        state.todos = items;
        if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
            return crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to save session state: {e}"),
            };
        }

        crate::soul::message::ToolReturnValue::Ok {
            output: format!("Saved {} todo item(s).", state.todos.len()),
            message: None,
        }
    }
}
