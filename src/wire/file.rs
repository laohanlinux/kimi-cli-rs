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
}
