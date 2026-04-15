use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Returns the plugins directory path.
pub fn get_plugins_dir() -> std::path::PathBuf {
    std::path::PathBuf::new()
}

/// Loads plugin tools from the given directory.
pub fn load_plugin_tools(
    _plugins_dir: &Path,
    _config: &crate::config::Config,
    _approval: &crate::soul::approval::Approval,
) -> Vec<Arc<dyn crate::soul::toolset::Tool>> {
    Vec::new()
}
