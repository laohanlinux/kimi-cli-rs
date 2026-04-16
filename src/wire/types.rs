use serde::{Deserialize, Serialize};

/// All messages that can travel over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    TurnBegin { user_input: String },
    StepBegin { step_no: usize },
    StepInterrupted,
    TurnEnd { stop_reason: String },
    CompactionBegin,
    CompactionEnd,
    StatusUpdate { snapshot: StatusSnapshot },
    Notification { text: String },
    PlanDisplay { content: String },
    BtwBegin { id: String, question: String },
    BtwEnd { id: String, response: Option<String>, error: Option<String> },
    SubagentEvent { agent_id: String, event: String },
    HookTriggered { hook_name: String },
    HookResolved { hook_name: String, duration_ms: u64 },
    McpLoadingBegin,
    McpLoadingEnd,

    TextPart { text: String },
    ThinkPart { thought: String },
    ImageUrlPart { url: String },
    AudioUrlPart { url: String },
    VideoUrlPart { url: String },

    ToolCall { tool_call_id: String, name: String, arguments: serde_json::Value },
    ToolCallPart { tool_call_id: String, index: usize, content: serde_json::Value },
    ToolResult { tool_call_id: String, result: crate::soul::message::ToolReturnValue },
    ApprovalResponse { request_id: String, response: String, feedback: Option<String> },

    ApprovalRequest {
        id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Option<serde_json::Value>,
    },
    QuestionRequest {
        id: String,
        items: Vec<QuestionItem>,
    },
    QuestionResponse {
        request_id: String,
        answers: std::collections::HashMap<String, String>,
    },
    ToolCallRequest {
        id: String,
        tool_call_id: String,
        name: String,
        arguments: serde_json::Value,
    },
    HookRequest {
        id: String,
        hook_name: String,
        input_data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub context_usage: f64,
    pub yolo_enabled: bool,
    pub plan_mode: bool,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_status: Option<crate::mcp::server::McpStatusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItem {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}
