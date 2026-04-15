use std::path::PathBuf;

/// Persistent wire message log file wrapper.
#[derive(Debug, Clone)]
pub struct WireFile {
    pub path: PathBuf,
}

impl WireFile {
    /// Creates a new wire file handle.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns true if the file is missing or empty.
    pub fn is_empty(&self) -> bool {
        match std::fs::metadata(&self.path) {
            Ok(m) => m.len() == 0,
            Err(_) => true,
        }
    }

    /// Reads all wire message records from the file.
    pub fn records(&self) -> Vec<crate::wire::types::WireMessage> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        text.lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}
