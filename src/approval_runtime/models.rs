use serde::{Deserialize, Serialize};

pub type ApprovalResponseKind = String;
pub type ApprovalSourceKind = String;
pub type ApprovalStatus = String;
pub type ApprovalRuntimeEventKind = String;

/// Source of an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalSource {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
}

impl ApprovalSource {
    pub fn foreground_turn(id: impl Into<String>) -> Self {
        Self {
            kind: "foreground_turn".into(),
            id: id.into(),
            agent_id: None,
            subagent_type: None,
        }
    }
}

/// A single approval request record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestRecord {
    pub id: String,
    pub tool_call_id: String,
    pub sender: String,
    pub action: String,
    pub description: String,
    pub display: Vec<serde_json::Value>,
    pub source: ApprovalSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<f64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    pub feedback: String,
}

impl ApprovalRequestRecord {
    pub fn new(
        id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Vec<serde_json::Value>,
        source: ApprovalSource,
    ) -> Self {
        Self {
            id,
            tool_call_id,
            sender,
            action,
            description,
            display,
            source,
            created_at: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            ),
            status: "pending".into(),
            resolved_at: None,
            response: None,
            feedback: String::new(),
        }
    }
}

/// Event emitted by the approval runtime.
#[derive(Debug, Clone)]
pub struct ApprovalRuntimeEvent {
    pub kind: String,
    pub request: ApprovalRequestRecord,
}

/// Result of evaluating approval rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Deny { reason: String },
    RequestUser,
}
