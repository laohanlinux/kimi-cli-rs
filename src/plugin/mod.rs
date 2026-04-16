use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Returns the default plugins directory path (`~/.kimi/plugins`).
pub fn get_plugins_dir() -> std::path::PathBuf {
    crate::share::get_share_dir()
        .map(|d| d.join("plugins"))
        .unwrap_or_else(|_| PathBuf::from(".").join("plugins"))
}

/// Plugin manifest definition.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub tools: Vec<PluginToolDef>,
}

/// Definition of a single plugin tool.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginToolDef {
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// A tool backed by a plugin shell command.
#[derive(Debug, Clone)]
pub struct PluginTool {
    name: String,
    description: String,
    command_template: String,
    parameters_schema: serde_json::Value,
}

impl PluginTool {
    fn new(def: &PluginToolDef) -> Self {
        let parameters_schema = if def.parameters.is_object() {
            def.parameters.clone()
        } else {
            serde_json::json!({
                "type": "object",
                "properties": {},
            })
        };
        Self {
            name: def.name.clone(),
            description: def.description.clone(),
            command_template: def.command.clone(),
            parameters_schema,
        }
    }

    /// Substitutes `{arg_name}` placeholders in the command template.
    fn build_command(&self, arguments: &serde_json::Value) -> String {
        let mut cmd = self.command_template.clone();
        if let Some(map) = arguments.as_object() {
            for (key, value) in map {
                let placeholder = format!("{{{key}}}");
                let replacement = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                cmd = cmd.replace(&placeholder, &replacement);
            }
        }
        cmd
    }
}

#[async_trait]
impl crate::soul::toolset::Tool for PluginTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.parameters_schema.clone()
    }

    #[tracing::instrument(level = "debug", skip(self, _runtime))]
    async fn call(
        &self,
        arguments: serde_json::Value,
        _runtime: &crate::soul::agent::Runtime,
    ) -> crate::soul::message::ToolReturnValue {
        let command = self.build_command(&arguments);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let output = tokio::process::Command::new(&shell)
            .arg("-c")
            .arg(&command)
            .output()
            .await;

        match output {
            Ok(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&stderr);
                }
                if out.status.success() {
                    crate::soul::message::ToolReturnValue::Ok {
                        output: text.trim_end().to_string(),
                        message: None,
                    }
                } else {
                    crate::soul::message::ToolReturnValue::Error {
                        error: format!("Plugin tool exited with code {:?}:\n{text}", out.status.code()),
                    }
                }
            }
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to execute plugin tool: {e}"),
            },
        }
    }
}

/// Loads plugin tools from the given directory.
///
/// Scans `plugins_dir` for subdirectories or `.toml` files, parses manifests,
/// and returns a vector of plugin-backed tools.
#[tracing::instrument(level = "debug", skip(_config, _approval))]
pub fn load_plugin_tools(
    plugins_dir: &Path,
    _config: &crate::config::Config,
    _approval: &crate::soul::approval::Approval,
) -> Vec<Arc<dyn crate::soul::toolset::Tool>> {
    if !plugins_dir.is_dir() {
        tracing::debug!(dir = %plugins_dir.display(), "plugins directory does not exist, skipping");
        return Vec::new();
    }

    let mut tools: Vec<Arc<dyn crate::soul::toolset::Tool>> = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read plugins directory: {}", e);
            return Vec::new();
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let manifest_path = if path.is_dir() {
            path.join("plugin.toml")
        } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            path
        } else {
            continue;
        };

        if !manifest_path.is_file() {
            continue;
        }

        let text = match std::fs::read_to_string(&manifest_path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to read plugin manifest {}: {}", manifest_path.display(), e);
                continue;
            }
        };

        let manifest: PluginManifest = match toml::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Failed to parse plugin manifest {}: {}", manifest_path.display(), e);
                continue;
            }
        };

        tracing::info!(
            plugin = %manifest.name,
            version = %manifest.version,
            tools = manifest.tools.len(),
            "loaded plugin manifest"
        );

        for def in &manifest.tools {
            if seen_names.contains(&def.name) {
                tracing::warn!(
                    plugin = %manifest.name,
                    tool = %def.name,
                    "duplicate plugin tool name, skipping"
                );
                continue;
            }
            seen_names.insert(def.name.clone());
            tools.push(Arc::new(PluginTool::new(def)));
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_tool_build_command() {
        let tool = PluginTool {
            name: "Test".into(),
            description: "Test tool".into(),
            command_template: "echo {greeting} {name}".into(),
            parameters_schema: serde_json::json!({}),
        };
        let args = serde_json::json!({"greeting": "hello", "name": "world"});
        assert_eq!(tool.build_command(&args), "echo hello world");
    }

    #[test]
    fn plugin_tool_build_command_missing_placeholder() {
        let tool = PluginTool {
            name: "Test".into(),
            description: "Test tool".into(),
            command_template: "echo hello".into(),
            parameters_schema: serde_json::json!({}),
        };
        let args = serde_json::json!({"name": "world"});
        assert_eq!(tool.build_command(&args), "echo hello");
    }

    #[tokio::test]
    async fn plugin_tool_echo() {
        use crate::soul::toolset::Tool;
        let tool = PluginTool {
            name: "Echo".into(),
            description: "Echo tool".into(),
            command_template: "echo hello".into(),
            parameters_schema: serde_json::json!({}),
        };
        let rt = crate::soul::agent::Runtime::default();
        let result = tool.call(serde_json::json!({}), &rt).await;
        match result {
            crate::soul::message::ToolReturnValue::Ok { output, .. } => {
                assert!(output.contains("hello"));
            }
            _ => panic!("expected ok"),
        }
    }
}
