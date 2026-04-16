use serde::{Deserialize, Serialize};
use std::path::Path;

const STATE_FILE_NAME: &str = "state.json";
const LEGACY_METADATA_FILENAME: &str = "metadata.json";

/// Persistent per-session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub approval: ApprovalStateData,
    #[serde(default)]
    pub additional_dirs: Vec<String>,
    #[serde(default)]
    pub custom_title: Option<String>,
    #[serde(default)]
    pub title_generated: bool,
    #[serde(default)]
    pub title_generate_attempts: u32,
    #[serde(default)]
    pub plan_mode: bool,
    #[serde(default)]
    pub plan_session_id: Option<String>,
    #[serde(default)]
    pub plan_slug: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub archived_at: Option<f64>,
    #[serde(default)]
    pub auto_archive_exempt: bool,
    #[serde(default)]
    pub wire_mtime: Option<f64>,
    #[serde(default)]
    pub todos: Vec<TodoItemState>,
}

fn default_version() -> u32 {
    1
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            version: default_version(),
            approval: ApprovalStateData::default(),
            additional_dirs: Vec::new(),
            custom_title: None,
            title_generated: false,
            title_generate_attempts: 0,
            plan_mode: false,
            plan_session_id: None,
            plan_slug: None,
            archived: false,
            archived_at: None,
            auto_archive_exempt: false,
            wire_mtime: None,
            todos: Vec::new(),
        }
    }
}

/// Approval settings stored per session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalStateData {
    pub yolo: bool,
    #[serde(default)]
    pub auto_approve_actions: Vec<String>,
}

/// Status of a todo item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Done,
}

/// A single todo item persisted in session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemState {
    pub title: String,
    pub status: TodoStatus,
}

/// Loads session state from the session directory.
#[tracing::instrument(level = "debug")]
pub fn load_session_state(session_dir: &Path) -> SessionState {
    let path = session_dir.join(STATE_FILE_NAME);
    let mut state = if !path.exists() {
        SessionState::default()
    } else {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        match serde_json::from_str::<SessionState>(&text) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Corrupted state file, using defaults: {} (error: {})", path.display(), e);
                SessionState::default()
            }
        }
    };

    // One-time migration from legacy metadata.json (best-effort)
    match migrate_legacy_metadata(session_dir, &mut state) {
        LegacyMigration::Migrated => {
            let _ = save_session_state(&state, session_dir);
            let _ = std::fs::remove_file(session_dir.join(LEGACY_METADATA_FILENAME));
        }
        LegacyMigration::NoChange => {
            let _ = std::fs::remove_file(session_dir.join(LEGACY_METADATA_FILENAME));
        }
        LegacyMigration::Skip => {}
    }

    state
}

enum LegacyMigration {
    Migrated,
    NoChange,
    Skip,
}

fn migrate_legacy_metadata(session_dir: &Path, state: &mut SessionState) -> LegacyMigration {
    let metadata_file = session_dir.join(LEGACY_METADATA_FILENAME);
    if !metadata_file.exists() {
        return LegacyMigration::Skip;
    }
    let text = match std::fs::read_to_string(&metadata_file) {
        Ok(t) => t,
        Err(_) => return LegacyMigration::Skip,
    };
    let data: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return LegacyMigration::Skip,
    };

    let mut changed = false;

    if state.custom_title.is_none() {
        if let Some(title) = data.get("title").and_then(|v| v.as_str()) {
            if !title.is_empty() && title != "Untitled" {
                state.custom_title = Some(title.to_string());
                changed = true;
            }
        }
    }
    if !state.title_generated {
        if data.get("title_generated").and_then(|v| v.as_bool()) == Some(true) {
            state.title_generated = true;
            changed = true;
        }
    }
    if state.title_generate_attempts == 0 {
        if let Some(n) = data.get("title_generate_attempts").and_then(|v| v.as_u64()) {
            if n > 0 {
                state.title_generate_attempts = n as u32;
                changed = true;
            }
        }
    }
    if !state.archived {
        if data.get("archived").and_then(|v| v.as_bool()) == Some(true) {
            state.archived = true;
            changed = true;
        }
    }
    if state.archived_at.is_none() {
        if let Some(n) = data.get("archived_at").and_then(|v| v.as_f64()) {
            state.archived_at = Some(n);
            changed = true;
        }
    }
    if !state.auto_archive_exempt {
        if data.get("auto_archive_exempt").and_then(|v| v.as_bool()) == Some(true) {
            state.auto_archive_exempt = true;
            changed = true;
        }
    }
    if state.wire_mtime.is_none() {
        if let Some(n) = data.get("wire_mtime").and_then(|v| v.as_f64()) {
            state.wire_mtime = Some(n);
            changed = true;
        }
    }

    if changed {
        LegacyMigration::Migrated
    } else {
        LegacyMigration::NoChange
    }
}

/// Saves session state to the session directory.
#[tracing::instrument(level = "debug")]
pub fn save_session_state(state: &SessionState, session_dir: &Path) -> crate::error::Result<()> {
    let path = session_dir.join(STATE_FILE_NAME);
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
            title: "do thing".into(),
            status: TodoStatus::Pending,
        });
        save_session_state(&st, &dir).unwrap();
        let loaded = load_session_state(&dir);
        assert_eq!(loaded.custom_title, Some("test-session".into()));
        assert_eq!(loaded.todos.len(), 1);
        assert_eq!(loaded.todos[0].title, "do thing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn approval_state_data_default() {
        let a = ApprovalStateData::default();
        assert!(!a.yolo);
        assert!(a.auto_approve_actions.is_empty());
    }
}
