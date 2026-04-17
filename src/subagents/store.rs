use std::path::PathBuf;

/// Registry for subagent definitions with persistence.
#[derive(Debug, Clone)]
pub struct SubagentStore {
    root: PathBuf,
}

impl SubagentStore {
    /// Creates a store rooted in the session directory.
    pub fn new(session: &crate::session::Session) -> Self {
        Self {
            root: session.dir().join("subagents"),
        }
    }

    /// Returns the instance directory for a given subagent ID.
    pub fn instance_dir(&self, agent_id: &str) -> PathBuf {
        self.root.join(agent_id)
    }

    pub fn context_path(&self, agent_id: &str) -> PathBuf {
        self.instance_dir(agent_id).join("context.jsonl")
    }

    pub fn wire_path(&self, agent_id: &str) -> PathBuf {
        self.instance_dir(agent_id).join("wire.jsonl")
    }

    pub fn meta_path(&self, agent_id: &str) -> PathBuf {
        self.instance_dir(agent_id).join("meta.json")
    }

    pub fn prompt_path(&self, agent_id: &str) -> PathBuf {
        self.instance_dir(agent_id).join("prompt.txt")
    }

    pub fn output_path(&self, agent_id: &str) -> PathBuf {
        self.instance_dir(agent_id).join("output")
    }

    /// Creates a new subagent instance with initialized files.
    pub fn create_instance(
        &self,
        agent_id: &str,
        description: &str,
        launch_spec: crate::subagents::models::AgentLaunchSpec,
    ) -> crate::subagents::models::AgentInstanceRecord {
        let dir = self.instance_dir(agent_id);
        std::fs::create_dir_all(&dir).ok();
        std::fs::File::create(dir.join("context.jsonl")).ok();
        std::fs::File::create(dir.join("wire.jsonl")).ok();
        std::fs::File::create(dir.join("prompt.txt")).ok();
        std::fs::File::create(dir.join("output")).ok();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let record = crate::subagents::models::AgentInstanceRecord {
            agent_id: agent_id.to_string(),
            subagent_type: launch_spec.subagent_type.clone(),
            status: crate::subagents::models::SubagentStatus::Idle,
            description: description.to_string(),
            created_at: now,
            updated_at: now,
            last_task_id: None,
            launch_spec,
        };
        self.write_instance(&record);
        record
    }

    /// Writes an instance record to disk.
    pub fn write_instance(&self, record: &crate::subagents::models::AgentInstanceRecord) {
        let path = self.meta_path(&record.agent_id);
        if let Ok(json) = serde_json::to_string_pretty(record) {
            std::fs::write(path, json).ok();
        }
    }

    /// Loads an instance record if it exists.
    pub fn get_instance(
        &self,
        agent_id: &str,
    ) -> Option<crate::subagents::models::AgentInstanceRecord> {
        let path = self.meta_path(agent_id);
        if !path.exists() {
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Requires an instance record to exist.
    pub fn require_instance(
        &self,
        agent_id: &str,
    ) -> crate::error::Result<crate::subagents::models::AgentInstanceRecord> {
        self.get_instance(agent_id).ok_or_else(|| {
            crate::error::KimiCliError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Subagent instance not found: {agent_id}"),
            ))
        })
    }

    /// Updates specific fields of an instance record.
    pub fn update_instance(
        &self,
        agent_id: &str,
        status: Option<crate::subagents::models::SubagentStatus>,
        description: Option<String>,
        last_task_id: Option<Option<String>>,
    ) -> crate::error::Result<crate::subagents::models::AgentInstanceRecord> {
        let mut current = self.require_instance(agent_id)?;
        if let Some(s) = status {
            current.status = s;
        }
        if let Some(d) = description {
            current.description = d;
        }
        if let Some(l) = last_task_id {
            current.last_task_id = l;
        }
        current.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.write_instance(&current);
        Ok(current)
    }

    /// Lists all instance records sorted by updated_at descending.
    pub fn list_instances(&self) -> Vec<crate::subagents::models::AgentInstanceRecord> {
        let mut records = Vec::new();
        if !self.root.exists() {
            return records;
        }
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(_) => return records,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let meta = path.join("meta.json");
            if !meta.exists() {
                continue;
            }
            if let Some(record) =
                self.get_instance(path.file_name().and_then(|s| s.to_str()).unwrap_or(""))
            {
                records.push(record);
            }
        }
        records.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        records
    }

    /// Deletes an instance and its directory.
    pub fn delete_instance(&self, agent_id: &str) {
        let dir = self.instance_dir(agent_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}

impl Default for SubagentStore {
    fn default() -> Self {
        Self {
            root: crate::share::get_share_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("kimi"))
                .join("subagents"),
        }
    }
}
