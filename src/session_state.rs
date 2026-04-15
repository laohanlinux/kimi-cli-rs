use serde::{Deserialize, Serialize};
use std::path::Path;

/// Persistent per-session state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub approval: ApprovalStateData,
    pub additional_dirs: Vec<String>,
    pub custom_title: Option<String>,
    pub title_generated: bool,
    pub title_generate_attempts: u32,
    pub plan_mode: bool,
    pub plan_session_id: Option<String>,
    pub plan_slug: Option<String>,
    pub archived: bool,
    pub archived_at: Option<f64>,
    pub auto_archive_exempt: bool,
    pub wire_mtime: Option<f64>,
    pub todos: Vec<TodoItemState>,
}

/// Approval settings stored per session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalStateData {
    pub yolo: bool,
    pub auto_approve_actions: Vec<String>,
}

/// A single todo item persisted in session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemState {
    pub id: String,
    pub content: String,
    pub done: bool,
}

/// Loads session state from the session directory.
#[tracing::instrument(level = "debug")]
pub fn load_session_state(session_dir: &Path) -> SessionState {
    let path = session_dir.join("state.json");
    if !path.exists() {
        return SessionState::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Saves session state to the session directory.
#[tracing::instrument(level = "debug")]
pub fn save_session_state(state: &SessionState, session_dir: &Path) -> crate::error::Result<()> {
    let path = session_dir.join("state.json");
    let text = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_default() {
        let st = SessionState::default();
        assert!(!st.approval.yolo);
        assert!(st.additional_dirs.is_empty());
        assert_eq!(st.title_generate_attempts, 0);
    }

    #[test]
    fn session_state_save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("kimi-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut st = SessionState::default();
        st.custom_title = Some("test-session".into());
        st.todos.push(TodoItemState {
            id: "1".into(),
            content: "do thing".into(),
            done: false,
        });
        save_session_state(&st, &dir).unwrap();
        let loaded = load_session_state(&dir);
        assert_eq!(loaded.custom_title, Some("test-session".into()));
        assert_eq!(loaded.todos.len(), 1);
        assert_eq!(loaded.todos[0].content, "do thing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn approval_state_data_default() {
        let a = ApprovalStateData::default();
        assert!(!a.yolo);
        assert!(a.auto_approve_actions.is_empty());
    }
}
