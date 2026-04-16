use std::collections::HashMap;

/// Registry of built-in subagent types.
#[derive(Debug, Clone, Default)]
pub struct SubagentRegistry {
    builtin_types: HashMap<String, crate::subagents::models::AgentTypeDefinition>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtin_types(&self) -> &HashMap<String, crate::subagents::models::AgentTypeDefinition> {
        &self.builtin_types
    }

    pub fn add_builtin_type(&mut self, type_def: crate::subagents::models::AgentTypeDefinition) {
        self.builtin_types.insert(type_def.name.clone(), type_def);
    }

    pub fn get_builtin_type(&self, name: &str) -> Option<&crate::subagents::models::AgentTypeDefinition> {
        self.builtin_types.get(name)
    }

    pub fn require_builtin_type(&self, name: &str) -> crate::error::Result<&crate::subagents::models::AgentTypeDefinition> {
        self.builtin_types.get(name).ok_or_else(|| {
            crate::error::KimiCliError::Generic(format!("Builtin subagent type not found: {name}"))
        })
    }
}
