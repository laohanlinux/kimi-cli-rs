use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Result of loading a single plugin manifest.
#[derive(Debug, Clone)]
pub struct PluginLoadResult {
    pub manifest_path: PathBuf,
    pub plugin_name: String,
    pub tools_loaded: usize,
    pub error: Option<String>,
}

/// Manages the lifecycle of plugin tools.
#[derive(Clone)]
pub struct PluginManager {
    plugins_dir: PathBuf,
    loaded: Vec<PluginLoadResult>,
    tools: Vec<Arc<dyn crate::soul::toolset::Tool>>,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugins_dir", &self.plugins_dir)
            .field("loaded", &self.loaded)
            .field("tool_count", &self.tools.len())
            .finish()
    }
}

impl PluginManager {
    /// Creates a new manager for the default plugins directory.
    pub fn default_dir() -> Self {
        Self::new(crate::plugin::get_plugins_dir())
    }

    /// Creates a new manager for the given plugins directory.
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            loaded: Vec::new(),
            tools: Vec::new(),
        }
    }

    /// Returns the plugins directory.
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    /// Loads all plugins from disk.
    #[tracing::instrument(level = "debug", skip(self, config, approval))]
    pub fn load(
        &mut self,
        config: &crate::config::Config,
        approval: &crate::soul::approval::Approval,
    ) -> Vec<Arc<dyn crate::soul::toolset::Tool>> {
        self.tools = crate::plugin::load_plugin_tools(&self.plugins_dir, config, approval);
        self.loaded = self.build_load_results();
        self.tools.clone()
    }

    /// Reloads all plugins from disk, returning any errors.
    #[tracing::instrument(level = "info", skip(self, config, approval))]
    pub fn reload(
        &mut self,
        config: &crate::config::Config,
        approval: &crate::soul::approval::Approval,
    ) -> Vec<Arc<dyn crate::soul::toolset::Tool>> {
        self.loaded.clear();
        self.tools.clear();
        self.load(config, approval)
    }

    /// Returns the tools from the last load/reload.
    pub fn tools(&self) -> &[Arc<dyn crate::soul::toolset::Tool>] {
        &self.tools
    }

    /// Returns the load results from the last load/reload.
    pub fn load_results(&self) -> &[PluginLoadResult] {
        &self.loaded
    }

    /// Returns true if any plugin failed to load.
    pub fn had_errors(&self) -> bool {
        self.loaded.iter().any(|r| r.error.is_some())
    }

    fn build_load_results(&self) -> Vec<PluginLoadResult> {
        let mut results = Vec::new();
        if !self.plugins_dir.is_dir() {
            return results;
        }
        let entries = match std::fs::read_dir(&self.plugins_dir) {
            Ok(e) => e,
            Err(e) => {
                results.push(PluginLoadResult {
                    manifest_path: self.plugins_dir.clone(),
                    plugin_name: String::new(),
                    tools_loaded: 0,
                    error: Some(format!("Failed to read plugins directory: {e}")),
                });
                return results;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let manifest_path = if path.is_dir() {
                path.join("plugin.toml")
            } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                path.clone()
            } else {
                continue;
            };

            if !manifest_path.is_file() {
                continue;
            }

            let text = match std::fs::read_to_string(&manifest_path) {
                Ok(t) => t,
                Err(e) => {
                    results.push(PluginLoadResult {
                        manifest_path: manifest_path.clone(),
                        plugin_name: manifest_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                        tools_loaded: 0,
                        error: Some(format!("Failed to read manifest: {e}")),
                    });
                    continue;
                }
            };

            match toml::from_str::<crate::plugin::PluginManifest>(&text) {
                Ok(manifest) => {
                    results.push(PluginLoadResult {
                        manifest_path: manifest_path.clone(),
                        plugin_name: manifest.name.clone(),
                        tools_loaded: manifest.tools.len(),
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(PluginLoadResult {
                        manifest_path: manifest_path.clone(),
                        plugin_name: manifest_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                        tools_loaded: 0,
                        error: Some(format!("Failed to parse manifest: {e}")),
                    });
                }
            }
        }

        results
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::default_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_manager_default_uses_share_dir() {
        let mgr = PluginManager::default();
        assert!(mgr.plugins_dir().to_string_lossy().contains("plugins"));
    }

    #[test]
    fn plugin_manager_load_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = PluginManager::new(tmp.path().to_path_buf());
        let tools = mgr.load(&crate::config::Config::default(), &crate::soul::approval::Approval::default());
        assert!(tools.is_empty());
        assert!(mgr.load_results().is_empty());
    }

    #[test]
    fn plugin_manager_reload_clears_previous() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = PluginManager::new(tmp.path().to_path_buf());
        let _ = mgr.load(&crate::config::Config::default(), &crate::soul::approval::Approval::default());
        let _ = mgr.reload(&crate::config::Config::default(), &crate::soul::approval::Approval::default());
        assert!(mgr.tools().is_empty());
    }
}
