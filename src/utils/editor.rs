use std::path::Path;

/// Opens the given file in the user's preferred editor.
pub async fn open(path: &Path) -> crate::error::Result<()> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    let status = tokio::process::Command::new(&editor)
        .arg(path)
        .status()
        .await
        .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;

    if !status.success() {
        return Err(crate::error::KimiCliError::Generic(
            format!("Editor {editor} exited with status: {status}")
        ));
    }
    Ok(())
}
