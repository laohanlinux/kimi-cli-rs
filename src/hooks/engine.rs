/// Action returned by a hook trigger.
#[derive(Debug, Clone)]
pub enum HookAction {
    Allow,
    Block { reason: String },
}

/// Engine that executes registered hooks.
#[derive(Debug, Clone, Default)]
pub struct HookEngine;

impl HookEngine {
    /// Triggers the named hook.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn trigger(
        &self,
        _hook_name: &str,
        _tool_name: &str,
        _arguments: serde_json::Value,
    ) -> crate::error::Result<HookAction> {
        Ok(HookAction::Allow)
    }
}
