//! Persist REPL transcript next to the Kimi session (Claude Code `SessionStorage` analogue).

use std::path::{Path, PathBuf};

use tokio::fs;

use super::message::ReplMessage;

pub fn transcript_path(session_dir: &Path) -> PathBuf {
    session_dir.join("repl_transcript.json")
}

pub async fn load_transcript(session_dir: &Path) -> crate::error::Result<Vec<ReplMessage>> {
    let path = transcript_path(session_dir);
    if !path.is_file() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path).await?;
    let v: Vec<ReplMessage> = serde_json::from_str(&raw).map_err(|e| {
        crate::error::KimiCliError::Generic(format!("repl transcript {}: {e}", path.display()))
    })?;
    Ok(v)
}

pub async fn save_transcript(
    session_dir: &Path,
    messages: &[ReplMessage],
) -> crate::error::Result<()> {
    let path = transcript_path(session_dir);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(messages)?;
    fs::write(&tmp, raw).await?;
    fs::rename(&tmp, &path).await?;
    Ok(())
}
