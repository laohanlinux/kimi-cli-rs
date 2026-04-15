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
