use std::path::Path;

/// Writes text to a file atomically using a temp file + rename.
pub fn atomic_write(text: &str, path: &Path) -> crate::error::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = parent.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp_path, text)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Write JSON data to a file atomically using a temp file + rename.
pub fn atomic_json_write<T: serde::Serialize>(data: &T, path: &Path) -> crate::error::Result<()> {
    let text = serde_json::to_string_pretty(data)?;
    atomic_write(&text, path)
}

/// Reads and deserializes JSON from a file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> crate::error::Result<T> {
    let text = std::fs::read_to_string(path)?;
    let data = serde_json::from_str(&text)?;
    Ok(data)
}

/// Ensures the parent directory of the given path exists.
pub fn ensure_parent_dir(path: &Path) -> crate::error::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.txt");
        atomic_write("hello world", &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn atomic_json_write_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.json");
        let data = serde_json::json!({"key": "value"});
        atomic_json_write(&data, &path).unwrap();
        let read: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(read["key"], "value");
    }

    #[test]
    fn ensure_parent_dir_creates_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/c/file.txt");
        ensure_parent_dir(&path).unwrap();
        assert!(tmp.path().join("a/b/c").exists());
    }
}
