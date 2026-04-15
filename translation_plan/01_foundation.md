# Phase 1: Foundation Modules Translation Plan

## 1.1 `src/error.rs` (from `exception.py`)

**Python:**
```python
class KimiCLIException(Exception): ...
class ConfigError(KimiCLIException, ValueError): ...
class AgentSpecError(KimiCLIException, ValueError): ...
class InvalidToolError(KimiCLIException, ValueError): ...
class SystemPromptTemplateError(KimiCLIException, ValueError): ...
class MCPConfigError(KimiCLIException, ValueError): ...
class MCPRuntimeError(KimiCLIException, RuntimeError): ...
```

**Rust:**
```rust
use thiserror::Error;

/// Base error type for the Kimi CLI application.
#[derive(Error, Debug)]
pub enum KimiCliError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
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
    Serde(#[from] serde_json::Error),
}

/// Alias for fallible operations within the crate.
pub type Result<T> = std::result::Result<T, KimiCliError>;
```

## 1.2 `src/constant.rs` (from `constant.py`)

**Python:**
```python
NAME = "Kimi Code CLI"
VERSION = ...  # lazy via importlib.metadata
USER_AGENT = ...
```

**Rust:**
```rust
use once_cell::sync::Lazy;

/// Application name displayed to users.
pub const NAME: &str = "Kimi Code CLI";

/// Application version loaded from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User-Agent header sent with HTTP requests.
pub static USER_AGENT: Lazy<String> = Lazy::new(|| {
    format!("{NAME}/{VERSION}")
});
```

## 1.3 `src/share.rs` (from `share.py`)

**Python:**
```python
def get_share_dir() -> Path:
    path = Path(os.environ.get("KIMI_SHARE_DIR", "~/.kimi")).expanduser()
    path.mkdir(parents=True, exist_ok=True)
    return path
```

**Rust:**
```rust
use std::path::PathBuf;

/// Returns the Kimi share directory, defaulting to `~/.kimi`.
/// Creates the directory if it does not exist.
#[tracing::instrument]
pub fn get_share_dir() -> crate::Result<PathBuf> {
    let path = std::env::var("KIMI_SHARE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("home directory should be available")
                .join(".kimi")
        });
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
```

## 1.4 `src/config.rs` (from `config.py`)

**Strategy:** Replace Pydantic with `serde`-annotated structs and manual validation.

**Key translations:**
- `BaseModel` -> `#[derive(Debug, Clone, Serialize, Deserialize)]`
- `SecretStr` -> `secrecy::SecretString`
- `field_validator` -> `impl Config { pub fn validate(&self) -> Result<()> }`
- `AliasChoices` -> `serde(alias = "...")`
- `load_config()` -> async or sync file I/O with `toml::from_str`

**Rust skeleton:**
```rust
use serde::{Deserialize, Serialize};
use secrecy::{ExposeSecret, SecretString};
use std::path::{Path, PathBuf};
use std::collections::HashSet;

/// OAuth credential storage reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthRef {
    pub storage: String, // "keyring" | "file"
    pub key: String,
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProvider {
    pub r#type: String,
    pub base_url: String,
    #[serde(serialize_with = "serialize_secret")]
    pub api_key: SecretString,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub custom_headers: Option<std::collections::HashMap<String, String>>,
    pub oauth: Option<OAuthRef>,
}

fn serialize_secret<S>(secret: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(secret.expose_secret())
}

/// Model capability flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    ImageIn,
    VideoIn,
    Thinking,
    AlwaysThinking,
}

/// LLM model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModel {
    pub provider: String,
    pub model: String,
    pub max_context_size: usize,
    pub capabilities: Option<HashSet<ModelCapability>>,
}

/// Agent loop control settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopControl {
    #[serde(default = "default_max_steps", alias = "max_steps_per_run")]
    pub max_steps_per_turn: usize,
    #[serde(default = "default_max_retries")]
    pub max_retries_per_step: usize,
    #[serde(default = "default_max_ralph")]
    pub max_ralph_iterations: isize,
    #[serde(default = "default_reserved_context")]
    pub reserved_context_size: usize,
    #[serde(default = "default_compaction_ratio")]
    pub compaction_trigger_ratio: f64,
}

fn default_max_steps() -> usize { 100 }
fn default_max_retries() -> usize { 3 }
fn default_max_ralph() -> isize { 0 }
fn default_reserved_context() -> usize { 50_000 }
fn default_compaction_ratio() -> f64 { 0.85 }

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing)]
    pub is_from_default_location: bool,
    #[serde(default, skip_serializing)]
    pub source_file: Option<PathBuf>,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub default_thinking: bool,
    #[serde(default)]
    pub default_yolo: bool,
    #[serde(default)]
    pub default_plan_mode: bool,
    #[serde(default)]
    pub default_editor: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub models: std::collections::HashMap<String, LlmModel>,
    #[serde(default)]
    pub providers: std::collections::HashMap<String, LlmProvider>,
    #[serde(default)]
    pub loop_control: LoopControl,
    #[serde(default)]
    pub background: BackgroundConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub services: Services,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub hooks: Vec<HookDef>,
    #[serde(default)]
    pub merge_all_available_skills: bool,
}

fn default_theme() -> String { "dark".into() }

impl Config {
    /// Validates consistency between default model, models, and providers.
    #[tracing::instrument(skip(self), level = "debug")]
    pub fn validate(&self) -> crate::Result<()> {
        if !self.default_model.is_empty() && !self.models.contains_key(&self.default_model) {
            return Err(crate::error::KimiCliError::Config(
                format!("default model '{}' not found in models", self.default_model).into()
            ));
        }
        for (name, model) in &self.models {
            if !self.providers.contains_key(&model.provider) {
                return Err(crate::error::KimiCliError::Config(
                    format!("provider '{}' for model '{}' not found", model.provider, name).into()
                ));
            }
        }
        Ok(())
    }
}

/// Loads configuration from the given path, or the default location.
#[tracing::instrument(level = "debug")]
pub fn load_config(path: Option<&Path>) -> crate::Result<Config> {
    let default_path = get_share_dir()?.join("config.toml");
    let config_path = path.unwrap_or(&default_path);
    let is_default = config_path == default_path;

    if !config_path.exists() {
        let config = Config {
            is_from_default_location: is_default,
            source_file: Some(config_path.to_path_buf()),
            ..Default::default()
        };
        save_config(&config, Some(config_path))?;
        return Ok(config);
    }

    let text = std::fs::read_to_string(config_path)?;
    let mut config: Config = if config_path.extension().and_then(|s| s.to_str()) == Some("json") {
        serde_json::from_str(&text)?
    } else {
        toml::from_str(&text)?
    };
    config.is_from_default_location = is_default;
    config.source_file = Some(config_path.to_path_buf());
    config.validate()?;
    Ok(config)
}

#[tracing::instrument(skip(config), level = "debug")]
pub fn save_config(config: &Config, path: Option<&Path>) -> crate::Result<()> {
    let path = path.unwrap_or(&get_share_dir()?.join("config.toml"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config)?;
    std::fs::write(path, text)?;
    Ok(())
}
```

## 1.5 `src/metadata.rs` (from `metadata.py`)

**Rust:**
```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metadata for a single work directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkDirMeta {
    pub path: String,
    pub kaos: String,
    pub last_session_id: Option<String>,
}

impl WorkDirMeta {
    /// Stable sessions directory based on MD5 hash of the path.
    pub fn sessions_dir(&self) -> PathBuf {
        let hash = format!("{:x}", md5::compute(&self.path));
        crate::share::get_share_dir().unwrap().join("sessions").join(hash)
    }
}

/// Global metadata index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub work_dirs: Vec<WorkDirMeta>,
}

impl Metadata {
    pub fn get_work_dir_meta(&self, path: &PathBuf) -> Option<&WorkDirMeta> {
        let canonical = dunce::canonicalize(path).ok()?;
        self.work_dirs.iter().find(|wd| {
            dunce::canonicalize(&wd.path).ok() == Some(canonical.clone())
        })
    }

    pub fn get_work_dir_meta_mut(&mut self, path: &PathBuf) -> Option<&mut WorkDirMeta> {
        let canonical = dunce::canonicalize(path).ok()?;
        self.work_dirs.iter_mut().find(|wd| {
            dunce::canonicalize(&wd.path).ok() == Some(canonical.clone())
        })
    }
}

#[tracing::instrument(level = "debug")]
pub fn load_metadata() -> Metadata {
    let path = crate::share::get_share_dir().unwrap().join("kimi.json");
    if !path.exists() {
        return Metadata::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

#[tracing::instrument(level = "debug")]
pub fn save_metadata(metadata: &Metadata) -> crate::Result<()> {
    let path = crate::share::get_share_dir()?.join("kimi.json");
    let text = serde_json::to_string_pretty(metadata)?;
    std::fs::write(&path, text)?;
    Ok(())
}
```

## 1.6 `src/session_state.rs` (from `session_state.py`)

**Rust:**
```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Persistent per-session state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub approval: ApprovalStateData,
    pub additional_dirs: Vec<String>,
    pub custom_title: Option<String>,
    pub title_generated: bool,
    pub title_generate_attempts: u32,
    pub plan_mode: bool,
    pub plan_session_id: Option<String>,
    pub plan_slug: Option<String>,
    pub archived: bool,
    pub archived_at: Option<f64>,
    pub auto_archive_exempt: bool,
    pub wire_mtime: Option<f64>,
    pub todos: Vec<TodoItemState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalStateData {
    pub yolo: bool,
    pub auto_approve_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemState {
    pub id: String,
    pub content: String,
    pub done: bool,
}

#[tracing::instrument(level = "debug")]
pub fn load_session_state(session_dir: &Path) -> SessionState {
    let path = session_dir.join("state.json");
    if !path.exists() {
        return SessionState::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

#[tracing::instrument(level = "debug")]
pub fn save_session_state(state: &SessionState, session_dir: &Path) -> crate::Result<()> {
    let path = session_dir.join("state.json");
    let text = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, text)?;
    Ok(())
}
```

## 1.7 `src/session.rs` (from `session.py`)

**Rust:**
```rust
use std::path::{Path, PathBuf};
use tokio::fs;

/// A single work-directory session.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub work_dir: PathBuf,
    pub work_dir_meta: crate::metadata::WorkDirMeta,
    pub context_file: PathBuf,
    pub wire_file: crate::wire::file::WireFile,
    pub state: crate::session_state::SessionState,
    pub title: String,
    pub updated_at: f64,
}

impl Session {
    /// Returns the session directory, creating it if necessary.
    pub fn dir(&self) -> PathBuf {
        let path = self.work_dir_meta.sessions_dir().join(&self.id);
        std::fs::create_dir_all(&path).ok();
        path
    }

    pub fn subagents_dir(&self) -> PathBuf {
        let path = self.dir().join("subagents");
        std::fs::create_dir_all(&path).ok();
        path
    }

    /// Saves mutable state to disk after reloading external fields.
    #[tracing::instrument(level = "debug")]
    pub fn save_state(&mut self) -> crate::Result<()> {
        let fresh = crate::session_state::load_session_state(&self.dir());
        self.state.custom_title = fresh.custom_title;
        self.state.title_generated = fresh.title_generated;
        self.state.title_generate_attempts = fresh.title_generate_attempts;
        self.state.archived = fresh.archived;
        self.state.archived_at = fresh.archived_at;
        self.state.auto_archive_exempt = fresh.auto_archive_exempt;
        crate::session_state::save_session_state(&self.state, &self.dir())
    }

    #[tracing::instrument(level = "debug")]
    pub async fn delete(&self) -> crate::Result<()> {
        let dir = self.dir();
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }
}
```

## Tracing Strategy for Foundation
- Every `load_*` / `save_*` function gets `#[tracing::instrument]`.
- `Session::create` and `Session::find` get explicit `info!` logs with IDs.
- Use `tracing::debug!` for configuration parsing details.
