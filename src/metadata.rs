use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const LOCAL_KAOS: &str = "local";

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
        let dir_basename = if self.kaos == LOCAL_KAOS {
            hash
        } else {
            format!("{}_{}", self.kaos, hash)
        };
        crate::share::get_share_dir()
            .unwrap()
            .join("sessions")
            .join(dir_basename)
    }
}

/// Global metadata index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub work_dirs: Vec<WorkDirMeta>,
}

impl Metadata {
    /// Finds metadata for the given work directory.
    pub fn get_work_dir_meta(&self, path: &PathBuf) -> Option<&WorkDirMeta> {
        let canonical = dunce::canonicalize(path).ok()?;
        self.work_dirs
            .iter()
            .find(|wd| dunce::canonicalize(&wd.path).ok() == Some(canonical.clone()))
    }

    /// Mutable access to work directory metadata.
    pub fn get_work_dir_meta_mut(&mut self, path: &PathBuf) -> Option<&mut WorkDirMeta> {
        let canonical = dunce::canonicalize(path).ok()?;
        self.work_dirs
            .iter_mut()
            .find(|wd| dunce::canonicalize(&wd.path).ok() == Some(canonical.clone()))
    }

    /// Creates a new entry for a work directory.
    pub fn new_work_dir_meta(&mut self, path: PathBuf) -> &WorkDirMeta {
        let meta = WorkDirMeta {
            path: path.to_string_lossy().to_string(),
            kaos: "local".into(),
            last_session_id: None,
        };
        self.work_dirs.push(meta);
        self.work_dirs.last().unwrap()
    }
}

/// Loads the global metadata from disk.
#[tracing::instrument(level = "debug")]
pub fn load_metadata() -> Metadata {
    let path = crate::share::get_share_dir().unwrap().join("kimi.json");
    if !path.exists() {
        return Metadata::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Saves the global metadata to disk atomically.
#[tracing::instrument(level = "debug")]
pub fn save_metadata(metadata: &Metadata) -> crate::error::Result<()> {
    let path = crate::share::get_share_dir()?.join("kimi.json");
    crate::utils::io::atomic_json_write(metadata, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_dir_meta_sessions_dir_contains_md5() {
        let meta = WorkDirMeta {
            path: "/tmp/test".into(),
            kaos: "local".into(),
            last_session_id: None,
        };
        let dir = meta.sessions_dir();
        let file_name = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(file_name, format!("{:x}", md5::compute("/tmp/test")));
    }

    #[test]
    fn metadata_get_work_dir_meta_roundtrip() {
        let mut meta = Metadata::default();
        let path = std::env::current_dir().unwrap();
        meta.new_work_dir_meta(path.clone());
        assert!(meta.get_work_dir_meta(&path).is_some());
    }

    #[test]
    fn metadata_json_roundtrip() {
        let mut meta = Metadata::default();
        meta.new_work_dir_meta(PathBuf::from("/tmp"));
        let text = serde_json::to_string_pretty(&meta).unwrap();
        let loaded: Metadata = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.work_dirs.len(), 1);
    }

    #[test]
    fn load_metadata_missing_file_returns_default() {
        // Ensure a random non-existent share dir yields default metadata
        let tmp = std::env::temp_dir().join(format!("kimi-meta-{}", uuid::Uuid::new_v4()));
        unsafe { std::env::set_var("KIMI_SHARE_DIR", &tmp) };
        let loaded = load_metadata();
        assert!(loaded.work_dirs.is_empty());
    }
}
