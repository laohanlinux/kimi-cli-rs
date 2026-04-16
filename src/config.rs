use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Reference to OAuth credentials stored outside the config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthRef {
    pub storage: String,
    pub key: String,
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProvider {
    pub r#type: String,
    pub base_url: String,
    #[serde(serialize_with = "serialize_secret")]
    pub api_key: SecretString,
    pub env: Option<HashMap<String, String>>,
    pub custom_headers: Option<HashMap<String, String>>,
    pub oauth: Option<OAuthRef>,
}

fn serialize_secret<S>(secret: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(secret.expose_secret())
}

/// Model capability flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ModelCapability {
    ImageIn,
    VideoIn,
    Thinking,
    AlwaysThinking,
}

/// Terminal color theme.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, strum::Display, strum::EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

fn default_max_steps() -> usize {
    100
}
fn default_max_retries() -> usize {
    3
}
fn default_max_ralph() -> isize {
    0
}
fn default_reserved_context() -> usize {
    50_000
}
fn default_compaction_ratio() -> f64 {
    0.85
}

/// Background task runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundConfig {
    #[serde(default = "default_max_running_tasks")]
    pub max_running_tasks: usize,
    #[serde(default = "default_read_max_bytes")]
    pub read_max_bytes: usize,
    #[serde(default = "default_notification_tail_lines")]
    pub notification_tail_lines: usize,
    #[serde(default = "default_notification_tail_chars")]
    pub notification_tail_chars: usize,
    #[serde(default = "default_wait_poll_interval_ms")]
    pub wait_poll_interval_ms: usize,
    #[serde(default = "default_worker_heartbeat_interval_ms")]
    pub worker_heartbeat_interval_ms: usize,
    #[serde(default = "default_worker_stale_after_ms")]
    pub worker_stale_after_ms: usize,
    #[serde(default = "default_kill_grace_period_ms")]
    pub kill_grace_period_ms: usize,
    #[serde(default)]
    pub keep_alive_on_exit: bool,
    #[serde(default = "default_agent_task_timeout_s")]
    pub agent_task_timeout_s: usize,
}

fn default_max_running_tasks() -> usize { 4 }
fn default_read_max_bytes() -> usize { 30_000 }
fn default_notification_tail_lines() -> usize { 20 }
fn default_notification_tail_chars() -> usize { 3_000 }
fn default_wait_poll_interval_ms() -> usize { 500 }
fn default_worker_heartbeat_interval_ms() -> usize { 5_000 }
fn default_worker_stale_after_ms() -> usize { 15_000 }
fn default_kill_grace_period_ms() -> usize { 2_000 }
fn default_agent_task_timeout_s() -> usize { 900 }

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            max_running_tasks: default_max_running_tasks(),
            read_max_bytes: default_read_max_bytes(),
            notification_tail_lines: default_notification_tail_lines(),
            notification_tail_chars: default_notification_tail_chars(),
            wait_poll_interval_ms: default_wait_poll_interval_ms(),
            worker_heartbeat_interval_ms: default_worker_heartbeat_interval_ms(),
            worker_stale_after_ms: default_worker_stale_after_ms(),
            kill_grace_period_ms: default_kill_grace_period_ms(),
            keep_alive_on_exit: false,
            agent_task_timeout_s: default_agent_task_timeout_s(),
        }
    }
}

/// Notification runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_claim_stale_after_ms")]
    pub claim_stale_after_ms: usize,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub desktop: bool,
    #[serde(default)]
    pub sound: bool,
}

fn default_claim_stale_after_ms() -> usize { 15_000 }
fn default_true() -> bool { true }

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            claim_stale_after_ms: default_claim_stale_after_ms(),
            enabled: default_true(),
            desktop: false,
            sound: false,
        }
    }
}

/// Moonshot Search configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoonshotSearchConfig {
    pub base_url: String,
    #[serde(serialize_with = "serialize_secret")]
    pub api_key: SecretString,
    pub custom_headers: Option<HashMap<String, String>>,
    pub oauth: Option<OAuthRef>,
}

/// Moonshot Fetch configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoonshotFetchConfig {
    pub base_url: String,
    #[serde(serialize_with = "serialize_secret")]
    pub api_key: SecretString,
    pub custom_headers: Option<HashMap<String, String>>,
    pub oauth: Option<OAuthRef>,
}

/// Services configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Services {
    pub moonshot_search: Option<MoonshotSearchConfig>,
    pub moonshot_fetch: Option<MoonshotFetchConfig>,
}

/// MCP client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientConfig {
    #[serde(default = "default_tool_call_timeout_ms")]
    pub tool_call_timeout_ms: usize,
}

fn default_tool_call_timeout_ms() -> usize { 60_000 }

impl Default for McpClientConfig {
    fn default() -> Self {
        Self {
            tool_call_timeout_ms: default_tool_call_timeout_ms(),
        }
    }
}

/// MCP server transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    /// stdio transport: command-based subprocess.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// HTTP/SSE transport: remote URL.
    Http {
        url: String,
        #[serde(default = "default_http_transport")]
        transport: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth: Option<String>,
    },
}

fn default_http_transport() -> String {
    "http".into()
}

/// MCP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub client: McpClientConfig,
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            client: McpClientConfig::default(),
            servers: HashMap::new(),
        }
    }
}

/// Hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    pub event: String,
    pub command: String,
    pub matcher: Option<String>,
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
}

fn default_hook_timeout() -> u64 { 30 }

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
    pub theme: Theme,
    #[serde(default)]
    pub models: HashMap<String, LlmModel>,
    #[serde(default)]
    pub providers: HashMap<String, LlmProvider>,
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

fn default_theme() -> Theme {
    Theme::Dark
}

impl Default for Config {
    fn default() -> Self {
        Self {
            is_from_default_location: false,
            source_file: None,
            default_model: String::new(),
            default_thinking: false,
            default_yolo: false,
            default_plan_mode: false,
            default_editor: String::new(),
            theme: Theme::Dark,
            models: HashMap::new(),
            providers: HashMap::new(),
            loop_control: LoopControl {
                max_steps_per_turn: default_max_steps(),
                max_retries_per_step: default_max_retries(),
                max_ralph_iterations: default_max_ralph(),
                reserved_context_size: default_reserved_context(),
                compaction_trigger_ratio: default_compaction_ratio(),
            },
            background: BackgroundConfig::default(),
            notifications: NotificationConfig::default(),
            services: Services::default(),
            mcp: McpConfig::default(),
            hooks: Vec::new(),
            merge_all_available_skills: false,
        }
    }
}

impl Config {
    /// Validates consistency between default model, models, and providers.
    #[tracing::instrument(skip(self), level = "debug")]
    pub fn validate(&self) -> crate::error::Result<()> {
        if !self.default_model.is_empty() && !self.models.contains_key(&self.default_model) {
            return Err(crate::error::KimiCliError::Config(format!(
                "default model '{}' not found in models",
                self.default_model
            )));
        }
        for (name, model) in &self.models {
            if !self.providers.contains_key(&model.provider) {
                return Err(crate::error::KimiCliError::Config(format!(
                    "provider '{}' for model '{}' not found",
                    model.provider, name
                )));
            }
        }
        Ok(())
    }
}

/// Returns the default configuration file path.
pub fn get_config_file() -> crate::error::Result<PathBuf> {
    Ok(crate::share::get_share_dir()?.join("config.toml"))
}

/// Loads configuration from the given path, or the default location.
#[tracing::instrument(level = "debug")]
pub fn load_config(path: Option<&Path>) -> crate::error::Result<Config> {
    let default_path = get_config_file()?;
    let config_path = path.unwrap_or(&default_path);
    let is_default = config_path == default_path.as_path();

    if is_default && !config_path.exists() {
        migrate_json_config_to_toml()?;
    }

    if !config_path.exists() {
        let mut config = Config::default();
        config.is_from_default_location = is_default;
        config.source_file = Some(config_path.to_path_buf());
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

/// Loads configuration from a TOML or JSON string.
#[tracing::instrument(level = "debug")]
pub fn load_config_from_string(config_string: &str) -> crate::error::Result<Config> {
    if config_string.trim().is_empty() {
        return Err(crate::error::KimiCliError::Config(
            "Configuration text cannot be empty".into(),
        ));
    }
    let mut config: Config = match serde_json::from_str(config_string) {
        Ok(c) => c,
        Err(json_err) => match toml::from_str(config_string) {
            Ok(c) => c,
            Err(toml_err) => {
                return Err(crate::error::KimiCliError::Config(format!(
                    "Invalid configuration text: {json_err}; {toml_err}"
                )))
            }
        },
    };
    config.is_from_default_location = false;
    config.source_file = None;
    config.validate()?;
    Ok(config)
}

/// Saves configuration to the given path, or the default path.
#[tracing::instrument(skip(config), level = "debug")]
pub fn save_config(config: &Config, path: Option<&Path>) -> crate::error::Result<()> {
    let default_path = get_config_file()?;
    let path = path.unwrap_or(&default_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_json = path.extension().and_then(|s| s.to_str()) == Some("json");
    if is_json {
        let data = serde_json::to_value(config)?;
        crate::utils::io::atomic_json_write(&data, path)?;
    } else {
        let text = toml::to_string_pretty(config)?;
        std::fs::write(path, text)?;
    }
    Ok(())
}

/// Migrates legacy JSON config to TOML.
fn migrate_json_config_to_toml() -> crate::error::Result<()> {
    let share_dir = crate::share::get_share_dir()?;
    let old_path = share_dir.join("config.json");
    let new_path = share_dir.join("config.toml");
    if !old_path.exists() || new_path.exists() {
        return Ok(());
    }
    tracing::info!("Migrating legacy config from {} to {}", old_path.display(), new_path.display());
    let text = std::fs::read_to_string(&old_path)?;
    let data: serde_json::Value = serde_json::from_str(&text)?;
    let config: Config = serde_json::from_value(data)?;
    save_config(&config, Some(&new_path))?;
    let backup = old_path.with_extension("json.bak");
    std::fs::rename(&old_path, &backup)?;
    tracing::info!("Legacy config backed up to {}", backup.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn config_default_is_valid() {
        let cfg = Config::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn config_validate_missing_default_model() {
        let mut cfg = Config::default();
        cfg.default_model = "missing".into();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("missing"));
    }

    #[test]
    fn config_validate_missing_provider() {
        let mut cfg = Config::default();
        cfg.models.insert(
            "test".into(),
            LlmModel {
                provider: "missing".into(),
                model: "m".into(),
                max_context_size: 1,
                capabilities: None,
            },
        );
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("missing"));
    }

    #[test]
    fn config_save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("kimi-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut cfg = Config::default();
        cfg.default_model = "kimi".into();
        cfg.models.insert(
            "kimi".into(),
            LlmModel {
                provider: "moonshot".into(),
                model: "kimi-k2".into(),
                max_context_size: 128_000,
                capabilities: Some(HashSet::from([ModelCapability::Thinking])),
            },
        );
        cfg.providers.insert(
            "moonshot".into(),
            LlmProvider {
                r#type: "openai".into(),
                base_url: "https://api.moonshot.cn".into(),
                api_key: SecretString::new("sk-test".into()),
                env: None,
                custom_headers: None,
                oauth: None,
            },
        );

        save_config(&cfg, Some(&path)).unwrap();
        let loaded = load_config(Some(&path)).unwrap();
        assert_eq!(loaded.default_model, "kimi");
        assert!(loaded.models.contains_key("kimi"));
        assert!(loaded.providers.contains_key("moonshot"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_config_from_string_roundtrip() {
        let mut cfg = Config::default();
        cfg.default_model = "kimi".into();
        cfg.models.insert(
            "kimi".into(),
            LlmModel {
                provider: "moonshot".into(),
                model: "kimi-k2".into(),
                max_context_size: 128_000,
                capabilities: None,
            },
        );
        cfg.providers.insert(
            "moonshot".into(),
            LlmProvider {
                r#type: "openai".into(),
                base_url: "https://api.moonshot.cn".into(),
                api_key: SecretString::new("sk-test".into()),
                env: None,
                custom_headers: None,
                oauth: None,
            },
        );

        let toml = toml::to_string(&cfg).unwrap();
        let loaded = load_config_from_string(&toml).unwrap();
        assert_eq!(loaded.default_model, "kimi");
        assert!(!loaded.is_from_default_location);
        assert!(loaded.source_file.is_none());

        let json = serde_json::to_string(&cfg).unwrap();
        let loaded_json = load_config_from_string(&json).unwrap();
        assert_eq!(loaded_json.default_model, "kimi");
    }

    #[test]
    fn load_config_from_string_empty_error() {
        let err = load_config_from_string("").unwrap_err().to_string();
        assert!(err.contains("empty"));
    }

    #[test]
    fn load_config_from_string_invalid_error() {
        let err = load_config_from_string("not valid toml or json").unwrap_err().to_string();
        assert!(err.contains("Invalid configuration text"));
    }

    #[test]
    fn model_capability_roundtrip() {
        let cap = ModelCapability::Thinking;
        let json = serde_json::to_string(&cap).unwrap();
        let de: ModelCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, de);
        assert_eq!(cap.to_string(), "thinking");
    }
}
