use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DEFAULT_AGENT_SPEC_VERSION: &str = "1";
const SUPPORTED_AGENT_SPEC_VERSIONS: [&str; 1] = [DEFAULT_AGENT_SPEC_VERSION];

/// Marker for inheritance in agent spec.
#[derive(Debug, Clone)]
pub struct Inherit;

/// Raw subagent specification.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubagentSpec {
    pub path: PathBuf,
    pub description: String,
}

/// Raw agent specification from YAML.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentSpec {
    pub extend: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "system_prompt_path")]
    pub system_prompt_path: Option<PathBuf>,
    #[serde(rename = "system_prompt_args", default)]
    pub system_prompt_args: HashMap<String, String>,
    pub model: Option<String>,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub exclude_tools: Option<Vec<String>>,
    #[serde(default)]
    pub subagents: HashMap<String, SubagentSpec>,
}

/// Resolved agent specification with inheritance flattened.
#[derive(Debug, Clone)]
pub struct ResolvedAgentSpec {
    pub name: String,
    pub system_prompt_path: PathBuf,
    pub system_prompt_args: HashMap<String, String>,
    pub model: Option<String>,
    pub when_to_use: String,
    pub tools: Vec<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub exclude_tools: Vec<String>,
    pub subagents: HashMap<String, SubagentSpec>,
}

/// Returns the built-in agents directory path.
pub fn get_agents_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("agents")
}

/// Default agent file path.
pub fn default_agent_file() -> PathBuf {
    get_agents_dir().join("default").join("agent.yaml")
}

/// Loads and resolves an agent specification from file.
#[tracing::instrument(level = "debug")]
pub fn load_agent_spec(agent_file: &Path) -> crate::error::Result<ResolvedAgentSpec> {
    let mut spec = _load_agent_spec(agent_file)?;
    if spec.extend.is_some() {
        return Err(crate::error::KimiCliError::Config(
            "Agent spec extension should be recursively resolved".into(),
        ));
    }
    let name = spec
        .name
        .ok_or_else(|| crate::error::KimiCliError::Config("Agent name is required".into()))?;
    let system_prompt_path = spec.system_prompt_path.ok_or_else(|| {
        crate::error::KimiCliError::Config("System prompt path is required".into())
    })?;
    let tools = spec
        .tools
        .ok_or_else(|| crate::error::KimiCliError::Config("Tools are required".into()))?;
    if spec.allowed_tools.is_none() {
        spec.allowed_tools = None;
    }
    if spec.exclude_tools.is_none() {
        spec.exclude_tools = Some(Vec::new());
    }
    if spec.subagents.is_empty() {
        // already empty, fine
    }
    Ok(ResolvedAgentSpec {
        name,
        system_prompt_path,
        system_prompt_args: spec.system_prompt_args,
        model: spec.model,
        when_to_use: spec.when_to_use.unwrap_or_default(),
        tools,
        allowed_tools: spec.allowed_tools,
        exclude_tools: spec.exclude_tools.unwrap_or_default(),
        subagents: spec.subagents,
    })
}

fn _load_agent_spec(agent_file: &Path) -> crate::error::Result<AgentSpec> {
    if !agent_file.exists() {
        return Err(crate::error::KimiCliError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Agent spec file not found: {}", agent_file.display()),
        )));
    }
    if !agent_file.is_file() {
        return Err(crate::error::KimiCliError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Agent spec path is not a file: {}", agent_file.display()),
        )));
    }
    let text = std::fs::read_to_string(agent_file)?;
    let data: serde_yaml::Value = serde_yaml::from_str(&text)?;
    let version = data
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_AGENT_SPEC_VERSION);
    if !SUPPORTED_AGENT_SPEC_VERSIONS.contains(&version) {
        return Err(crate::error::KimiCliError::Config(
            format!("Unsupported agent spec version: {version}").into(),
        ));
    }
    let mut spec: AgentSpec = serde_yaml::from_value(
        data.get("agent")
            .cloned()
            .unwrap_or(serde_yaml::Value::Null),
    )?;

    if let Some(ref mut path) = spec.system_prompt_path {
        *path = agent_file.parent().unwrap_or(Path::new(".")).join(&*path);
    }
    for subagent in spec.subagents.values_mut() {
        subagent.path = agent_file
            .parent()
            .unwrap_or(Path::new("."))
            .join(&subagent.path);
    }

    if let Some(ref extend) = spec.extend {
        let base_agent_file = if extend == "default" {
            default_agent_file()
        } else {
            agent_file.parent().unwrap_or(Path::new(".")).join(extend)
        };
        let mut base_spec = _load_agent_spec(&base_agent_file)?;
        if spec.name.is_some() {
            base_spec.name = spec.name.clone();
        }
        if spec.system_prompt_path.is_some() {
            base_spec.system_prompt_path = spec.system_prompt_path.clone();
        }
        for (k, v) in &spec.system_prompt_args {
            base_spec.system_prompt_args.insert(k.clone(), v.clone());
        }
        if spec.model.is_some() {
            base_spec.model = spec.model.clone();
        }
        if spec.when_to_use.is_some() {
            base_spec.when_to_use = spec.when_to_use.clone();
        }
        if spec.tools.is_some() {
            base_spec.tools = spec.tools.clone();
        }
        if spec.allowed_tools.is_some() {
            base_spec.allowed_tools = spec.allowed_tools.clone();
        }
        if spec.exclude_tools.is_some() {
            base_spec.exclude_tools = spec.exclude_tools.clone();
        }
        if !spec.subagents.is_empty() {
            base_spec.subagents = spec.subagents.clone();
        }
        spec = base_spec;
    }
    Ok(spec)
}
