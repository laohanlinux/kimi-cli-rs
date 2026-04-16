use async_trait::async_trait;
use serde::Deserialize;

/// A single todo item parameter.
#[derive(Debug, Clone, Deserialize)]
pub struct Todo {
    pub title: String,
    pub status: crate::session_state::TodoStatus,
}

/// Sets the todo list for the session.
#[derive(Debug, Clone, Default)]
pub struct SetTodoList;

#[async_trait]
impl crate::soul::toolset::Tool for SetTodoList {
    fn name(&self) -> &str {
        "SetTodoList"
    }

    fn description(&self) -> &str {
        "Set, update, or read the session todo list."
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
                            "title": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "done"] }
                        },
                        "required": ["title", "status"]
                    }
                }
            }
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        if let Some(arr) = arguments.get("todos").and_then(|v| v.as_array()) {
            let mut todos = Vec::new();
            for t in arr {
                if let Ok(todo) = serde_json::from_value::<Todo>(t.clone()) {
                    todos.push(crate::session_state::TodoItemState {
                        title: todo.title,
                        status: todo.status,
                    });
                }
            }

            if runtime.role == "root" {
                let session_dir = runtime.session.dir();
                let mut state = crate::session_state::load_session_state(&session_dir);
                state.todos = todos;
                if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to save session state: {e}"),
                    };
                }
            } else if let Some(ref store) = runtime.subagent_store {
                if let Some(ref agent_id) = runtime.subagent_id {
                    let state_file = store.instance_dir(agent_id).join("state.json");
                    let mut data = read_subagent_state(&state_file);
                    data["todos"] = serde_json::to_value(&todos).unwrap_or_default();
                    if let Err(e) = write_subagent_state(&state_file, &data) {
                        return crate::soul::message::ToolReturnValue::Error {
                            error: format!("Failed to save subagent state: {e}"),
                        };
                    }
                }
            }

            crate::soul::message::ToolReturnValue::Ok {
                output: format!("Saved {} todo item(s).", arr.len()),
                message: None,
            }
        } else {
            // Read mode
            let items = if runtime.role == "root" {
                let session_dir = runtime.session.dir();
                let state = crate::session_state::load_session_state(&session_dir);
                state.todos
            } else if let Some(ref store) = runtime.subagent_store {
                runtime
                    .subagent_id
                    .as_ref()
                    .map(|agent_id| {
                        let state_file = store.instance_dir(agent_id).join("state.json");
                        let data = read_subagent_state(&state_file);
                        data.get("todos")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|t| {
                                        serde_json::from_value::<crate::session_state::TodoItemState>(t.clone()).ok()
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            if items.is_empty() {
                crate::soul::message::ToolReturnValue::Ok {
                    output: "Todo list is empty.".into(),
                    message: None,
                }
            } else {
                let mut lines = vec!["Current todo list:".to_string()];
                for t in &items {
                    let status = format!("{:?}", t.status).to_lowercase();
                    lines.push(format!("- [{}] {}", status, t.title));
                }
                crate::soul::message::ToolReturnValue::Ok {
                    output: lines.join("\n"),
                    message: None,
                }
            }
        }
    }
}

fn read_subagent_state(path: &std::path::Path) -> serde_json::Value {
    if !path.exists() {
        return serde_json::json!({});
    }
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

fn write_subagent_state(
    path: &std::path::Path,
    data: &serde_json::Value,
) -> crate::error::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(data)?;
    std::fs::write(path, text)?;
    Ok(())
}
