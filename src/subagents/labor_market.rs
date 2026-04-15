use std::collections::HashMap;
use std::path::PathBuf;

/// Tool policy mode for subagents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolPolicyMode {
    #[default]
    Inherit,
    Allowlist,
}

/// Policy governing which tools a subagent may use.
#[derive(Debug, Clone, Default)]
pub struct ToolPolicy {
    pub mode: ToolPolicyMode,
    pub tools: Vec<String>,
}

impl ToolPolicy {
    pub fn allowlist(tools: Vec<String>) -> Self {
        Self {
            mode: ToolPolicyMode::Allowlist,
            tools,
        }
    }

    pub fn inherit() -> Self {
        Self {
            mode: ToolPolicyMode::Inherit,
            tools: Vec::new(),
        }
    }
}

/// Definition of a builtin subagent type.
#[derive(Debug, Clone)]
pub struct AgentTypeDefinition {
    pub name: String,
    pub description: String,
    pub agent_file: PathBuf,
    pub when_to_use: String,
    pub default_model: Option<String>,
    pub tool_policy: ToolPolicy,
    pub supports_background: bool,
}

/// Registry of built-in subagent types.
#[derive(Debug, Clone, Default)]
pub struct LaborMarket {
    builtin_types: HashMap<String, AgentTypeDefinition>,
}

impl LaborMarket {
    pub fn add_builtin_type(&mut self, type_def: AgentTypeDefinition) {
        self.builtin_types.insert(type_def.name.clone(), type_def);
    }

    pub fn get_builtin_type(&self, name: &str) -> Option<&AgentTypeDefinition> {
        self.builtin_types.get(name)
    }
}
