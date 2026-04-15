use crate::soul::dynamic_injection::{DynamicInjection, DynamicInjectionProvider};

/// Injects YOLO mode reminders when auto-approve is enabled.
#[derive(Debug, Clone, Default)]
pub struct YoloModeInjectionProvider;

#[async_trait::async_trait]
impl DynamicInjectionProvider for YoloModeInjectionProvider {
    #[tracing::instrument(level = "debug", skip(self, soul))]
    async fn get_injections(
        &self,
        _history: &[crate::soul::message::Message],
        soul: &crate::soul::kimisoul::KimiSoul,
    ) -> Vec<DynamicInjection> {
        if !soul.runtime.approval.yolo {
            return Vec::new();
        }
        vec![DynamicInjection {
            r#type: "yolo_mode".into(),
            content: "YOLO mode is enabled. All actions will be auto-approved.".into(),
        }]
    }
}
