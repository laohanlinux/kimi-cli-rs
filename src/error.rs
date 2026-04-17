use thiserror::Error;

/// Base error type for the Kimi CLI application.
#[derive(Error, Debug)]
pub enum KimiCliError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("agent spec error: {0}")]
    AgentSpec(String),
    #[error("invalid tool: {0}")]
    InvalidTool(String),
    #[error("system prompt template error: {0}")]
    SystemPromptTemplate(String),
    #[error("MCP config error: {0}")]
    McpConfig(String),
    #[error("MCP runtime error: {0}")]
    McpRuntime(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("YAML error: {0}")]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("approval timed out")]
    ApprovalTimeout,
    #[error("approval cancelled")]
    ApprovalCancelled,
    #[error("LLM not set")]
    LlmNotSet,
    #[error("LLM not supported: {0}")]
    LlmNotSupported(String),
    #[error("run cancelled")]
    RunCancelled,
    #[error("max steps reached")]
    MaxStepsReached,
    #[error("OAuth unauthorized")]
    OAuthUnauthorized,
    #[error("generic error: {0}")]
    Generic(String),
}

/// Alias for fallible operations within the crate.
pub type Result<T> = std::result::Result<T, KimiCliError>;

/// Error raised when a tool call is rejected by the user.
#[derive(Debug, Clone)]
pub struct ToolRejectedError {
    pub message: String,
    pub brief: String,
    pub has_feedback: bool,
}

impl std::fmt::Display for ToolRejectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolRejectedError {}

impl ToolRejectedError {
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            brief: "Rejected by user".into(),
            has_feedback: false,
            message,
        }
    }

    pub fn with_brief(mut self, brief: impl Into<String>) -> Self {
        self.brief = brief.into();
        self
    }

    pub fn with_feedback(mut self, feedback: impl Into<String>) -> Self {
        let feedback = feedback.into();
        self.has_feedback = true;
        self.message = format!(
            "The tool call is rejected by the user. User feedback: {}",
            feedback
        );
        self.brief = format!("Rejected: {}", feedback);
        self
    }

    pub fn for_subagent() -> Self {
        Self {
            message: ("The tool call is rejected by the user. ".to_string()
                + "Try a different approach to complete your task, or explain the "
                + "limitation in your summary if no alternative is available. "
                + "Do not retry the same tool call, and do not attempt to bypass "
                + "this restriction through indirect means."),
            brief: "Rejected by user".into(),
            has_feedback: false,
        }
    }
}

impl Default for ToolRejectedError {
    fn default() -> Self {
        Self {
            message: ("The tool call is rejected by the user. ".to_string()
                + "Stop what you are doing and wait for the user to tell you how to proceed."),
            brief: "Rejected by user".into(),
            has_feedback: false,
        }
    }
}
