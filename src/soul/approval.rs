use std::fmt;
use std::sync::Arc;

/// Result of an approval request. Behaves as bool for backward compatibility.
#[derive(Debug, Clone)]
pub struct ApprovalResult {
    pub approved: bool,
    pub feedback: String,
}

impl ApprovalResult {
    pub fn new(approved: bool, feedback: impl Into<String>) -> Self {
        Self {
            approved,
            feedback: feedback.into(),
        }
    }

    pub fn rejection_error(&self) -> crate::error::ToolRejectedError {
        if !self.feedback.is_empty() {
            return crate::error::ToolRejectedError::default().with_feedback(self.feedback.clone());
        }
        crate::error::ToolRejectedError::default()
    }
}

impl std::ops::Deref for ApprovalResult {
    type Target = bool;

    fn deref(&self) -> &Self::Target {
        &self.approved
    }
}

impl From<bool> for ApprovalResult {
    fn from(approved: bool) -> Self {
        Self::new(approved, "")
    }
}

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
    pub auto_approve_actions: std::collections::HashSet<String>,
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
            auto_approve_actions: auto_approve_actions.into_iter().collect(),
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
            auto_approve_actions: std::collections::HashSet::new(),
            on_change: ChangeCallback(None),
        }
    }
}

/// Approval manager for tool calls.
#[derive(Debug, Clone)]
pub struct Approval {
    state: Arc<tokio::sync::Mutex<ApprovalState>>,
    runtime: crate::approval_runtime::ApprovalRuntime,
}

impl Approval {
    pub fn new(
        yolo: bool,
        state: Option<ApprovalState>,
        runtime: Option<crate::approval_runtime::ApprovalRuntime>,
    ) -> Self {
        let state = state.unwrap_or_else(|| ApprovalState::new(yolo, Vec::new(), None));
        Self {
            state: Arc::new(tokio::sync::Mutex::new(state)),
            runtime: runtime.unwrap_or_default(),
        }
    }

    pub fn share(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            runtime: self.runtime.clone(),
        }
    }

    pub fn set_runtime(&mut self, runtime: crate::approval_runtime::ApprovalRuntime) {
        self.runtime = runtime;
    }

    pub fn runtime(&self) -> &crate::approval_runtime::ApprovalRuntime {
        &self.runtime
    }

    pub async fn set_yolo(&self, yolo: bool) {
        let mut state = self.state.lock().await;
        state.yolo = yolo;
        state.notify_change();
    }

    pub async fn is_yolo(&self) -> bool {
        self.state.lock().await.yolo
    }

    pub fn yolo_blocking(&self) -> bool {
        match self.state.try_lock() {
            Ok(guard) => guard.yolo,
            Err(_) => false,
        }
    }

    pub async fn request(
        &self,
        sender: &str,
        action: &str,
        description: &str,
        display: Option<Vec<serde_json::Value>>,
    ) -> crate::error::Result<ApprovalResult> {
        let tool_call = crate::soul::toolset::get_current_tool_call_or_none().ok_or_else(|| {
            crate::error::KimiCliError::Generic(
                "Approval must be requested from a tool call.".into(),
            )
        })?;

        tracing::debug!(
            tool_name = %tool_call.name,
            tool_call_id = %tool_call.id,
            action = %action,
            description = %description,
            "requesting approval"
        );

        {
            let state = self.state.lock().await;
            if state.yolo || state.auto_approve_actions.contains(action) {
                return Ok(ApprovalResult::new(true, ""));
            }
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let display_blocks = display.unwrap_or_default();
        let source = crate::approval_runtime::ApprovalSource::foreground_turn(&tool_call.id);

        self.runtime.create_request(
            request_id.clone(),
            tool_call.id.clone(),
            sender.into(),
            action.into(),
            description.into(),
            display_blocks,
            source,
        );

        match self.runtime.wait_for_response(&request_id, 300).await {
            Ok((response, feedback)) => match response.as_str() {
                "approve" => Ok(ApprovalResult::new(true, "")),
                "approve_for_session" => {
                    let mut state = self.state.lock().await;
                    state.auto_approve_actions.insert(action.into());
                    state.notify_change();
                    drop(state);
                    for pending in self.runtime.list_pending() {
                        if pending.action == action {
                            self.runtime.resolve(&pending.id, "approve", "");
                        }
                    }
                    Ok(ApprovalResult::new(true, ""))
                }
                "reject" => Ok(ApprovalResult::new(false, feedback)),
                _ => Ok(ApprovalResult::new(false, "")),
            },
            Err(crate::error::KimiCliError::ApprovalCancelled) => {
                Ok(ApprovalResult::new(false, ""))
            }
            Err(e) => Err(e),
        }
    }
}

impl Default for Approval {
    fn default() -> Self {
        Self::new(false, None, None)
    }
}
