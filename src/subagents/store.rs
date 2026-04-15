use std::path::PathBuf;

/// Registry for subagent definitions.
#[derive(Debug, Clone, Default)]
pub struct SubagentStore;

impl SubagentStore {
    /// Returns the instance directory for a given subagent ID.
    pub fn instance_dir(&self, agent_id: &str) -> PathBuf {
        crate::share::get_share_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("kimi"))
            .join("subagents")
            .join(agent_id)
    }
}
