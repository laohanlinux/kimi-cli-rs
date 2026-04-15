use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
struct ChangeCallback(Option<Arc<dyn Fn() + Send + Sync>>);

impl fmt::Debug for ChangeCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangeCallback").finish()
    }
}

/// Approval state and auto-approve settings.
#[derive(Debug, Clone)]
pub struct ApprovalState {
    pub yolo: bool,
    pub auto_approve_actions: Vec<String>,
    on_change: ChangeCallback,
}

impl ApprovalState {
    pub fn new(
        yolo: bool,
        auto_approve_actions: Vec<String>,
        on_change: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            yolo,
            auto_approve_actions,
            on_change: ChangeCallback(on_change),
        }
    }

    pub fn notify_change(&self) {
        if let Some(ref cb) = self.on_change.0 {
            cb();
        }
    }
}

impl Default for ApprovalState {
    fn default() -> Self {
        Self {
            yolo: false,
            auto_approve_actions: Vec::new(),
            on_change: ChangeCallback(None),
        }
    }
}

/// Approval manager for tool calls.
#[derive(Debug, Clone, Default)]
pub struct Approval {
    pub yolo: bool,
    pub auto_approve_actions: Vec<String>,
}

impl Approval {
    pub fn share(&self) -> Self {
        self.clone()
    }
}
