use serde::{Deserialize, Serialize};

/// A display block describing a file diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffDisplayBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub path: String,
    pub old_text: String,
    pub new_text: String,
    #[serde(default)]
    pub old_start: usize,
    #[serde(default = "default_new_start")]
    pub new_start: usize,
    #[serde(default)]
    pub is_summary: bool,
}

fn default_new_start() -> usize {
    1
}

impl DiffDisplayBlock {
    pub fn new(
        path: impl Into<String>,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
    ) -> Self {
        Self {
            block_type: "diff".into(),
            path: path.into(),
            old_text: old_text.into(),
            new_text: new_text.into(),
            old_start: 1,
            new_start: 1,
            is_summary: false,
        }
    }
}

/// A single item in a todo display block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoDisplayItem {
    pub title: String,
    pub status: TodoStatus,
}

/// Status of a todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

/// A display block describing a todo list update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoDisplayBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub items: Vec<TodoDisplayItem>,
}

impl TodoDisplayBlock {
    pub fn new(items: Vec<TodoDisplayItem>) -> Self {
        Self {
            block_type: "todo".into(),
            items,
        }
    }
}

/// A display block describing a shell command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellDisplayBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub language: String,
    pub command: String,
}

impl ShellDisplayBlock {
    pub fn new(language: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            block_type: "shell".into(),
            language: language.into(),
            command: command.into(),
        }
    }
}

/// A display block describing a background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskDisplayBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub task_id: String,
    pub kind: String,
    pub status: String,
    pub description: String,
}

impl BackgroundTaskDisplayBlock {
    pub fn new(
        task_id: impl Into<String>,
        kind: impl Into<String>,
        status: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            block_type: "background_task".into(),
            task_id: task_id.into(),
            kind: kind.into(),
            status: status.into(),
            description: description.into(),
        }
    }
}

/// A brief text display block for compact UI rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefDisplayBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

impl BriefDisplayBlock {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            block_type: "brief".into(),
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_display_block_serializes() {
        let block = DiffDisplayBlock::new("src/main.rs", "old", "new");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "diff");
        assert_eq!(json["path"], "src/main.rs");
    }

    #[test]
    fn todo_display_block_serializes() {
        let block = TodoDisplayBlock::new(vec![TodoDisplayItem {
            title: "test".into(),
            status: TodoStatus::Pending,
        }]);
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "todo");
        assert_eq!(json["items"][0]["title"], "test");
    }

    #[test]
    fn shell_display_block_serializes() {
        let block = ShellDisplayBlock::new("bash", "echo hello");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "shell");
        assert_eq!(json["command"], "echo hello");
    }

    #[test]
    fn background_task_display_block_serializes() {
        let block = BackgroundTaskDisplayBlock::new("t1", "bash", "running", "desc");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "background_task");
        assert_eq!(json["task_id"], "t1");
    }
}
