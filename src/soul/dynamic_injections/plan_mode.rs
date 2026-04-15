use crate::soul::dynamic_injection::{DynamicInjection, DynamicInjectionProvider};

/// Injects plan mode instructions when plan mode is active.
#[derive(Debug, Clone, Default)]
pub struct PlanModeInjectionProvider;

#[async_trait::async_trait]
impl DynamicInjectionProvider for PlanModeInjectionProvider {
    #[tracing::instrument(level = "debug", skip(self, soul))]
    async fn get_injections(
        &self,
        _history: &[crate::soul::message::Message],
        soul: &crate::soul::kimisoul::KimiSoul,
    ) -> Vec<DynamicInjection> {
        if !soul.plan_mode {
            return Vec::new();
        }
        vec![DynamicInjection {
            r#type: "plan_mode".into(),
            content: "Plan mode is active. Use research and planning tools only.".into(),
        }]
    }
}
