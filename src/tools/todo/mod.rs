use async_trait::async_trait;
use serde::Deserialize;

/// Todo item status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl Default for TodoStatus {
    fn default() -> Self {
        TodoStatus::Pending
    }
}

/// A single todo item.
#[derive(Debug, Clone, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub status: Option<TodoStatus>,
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
                            "id": { "type": "string" },
                            "content": { "type": "string" },
                            "done": { "type": "boolean" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "done"] }
                        },
                        "required": ["id", "content", "done"]
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
            let mut items = Vec::new();
            for t in arr {
                let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let done = t.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                items.push(crate::session_state::TodoItemState { id, content, done });
            }

            if runtime.role == "root" {
                let session_dir = runtime.session.dir();
                let mut state = crate::session_state::load_session_state(&session_dir);
                state.todos = items;
                if let Err(e) = crate::session_state::save_session_state(&state, &session_dir) {
                    return crate::soul::message::ToolReturnValue::Error {
                        error: format!("Failed to save session state: {e}"),
                    };
                }
            } else if let Some(ref store) = runtime.subagent_store {
                if let Some(ref agent_id) = runtime.subagent_id {
                    let state_file = store.instance_dir(agent_id).join("state.json");
                    let mut data = read_subagent_state(&state_file);
                    data["todos"] = serde_json::to_value(&items).unwrap_or_default();
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
                    let status = if t.done { "done" } else { "pending" };
                    lines.push(format!("  - [{}] {}", status, t.content));
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
