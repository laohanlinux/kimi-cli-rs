use std::path::Path;

/// Write JSON data to a file atomically using a temp file + rename.
pub fn atomic_json_write<T: serde::Serialize>(data: &T, path: &Path) -> crate::error::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = parent.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    let text = serde_json::to_string_pretty(data)?;
    std::fs::write(&tmp_path, text)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
