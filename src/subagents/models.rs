use serde::{Deserialize, Serialize};

/// Status of a subagent instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentStatus {
    Idle,
    RunningForeground,
    RunningBackground,
    Completed,
    Failed,
    Killed,
}

impl std::fmt::Display for SubagentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubagentStatus::Idle => write!(f, "idle"),
            SubagentStatus::RunningForeground => write!(f, "running_foreground"),
            SubagentStatus::RunningBackground => write!(f, "running_background"),
            SubagentStatus::Completed => write!(f, "completed"),
            SubagentStatus::Failed => write!(f, "failed"),
            SubagentStatus::Killed => write!(f, "killed"),
        }
    }
}

/// Specification for launching a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLaunchSpec {
    pub agent_id: String,
    pub subagent_type: String,
    pub model_override: Option<String>,
    pub effective_model: Option<String>,
    pub created_at: f64,
}

impl AgentLaunchSpec {
    pub fn new(
        agent_id: String,
        subagent_type: String,
        model_override: Option<String>,
        effective_model: Option<String>,
    ) -> Self {
        Self {
            agent_id,
            subagent_type,
            model_override,
            effective_model,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        }
    }
}

/// Record of a subagent instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstanceRecord {
    pub agent_id: String,
    pub subagent_type: String,
    pub status: SubagentStatus,
    pub description: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_task_id: Option<String>,
    pub launch_spec: AgentLaunchSpec,
}
