/// Copies the given text to the system clipboard.
pub fn copy(text: &str) -> crate::error::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut pb = std::process::Command::new("pbcopy");
        crate::utils::subprocess_env::apply_to_std(
            &mut pb,
            crate::utils::subprocess_env::get_clean_env(),
        );
        let mut child = pb
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;
        use std::io::Write;
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;
        }
        let status = child
            .wait()
            .map_err(|e| crate::error::KimiCliError::Io(e.into()))?;
        if !status.success() {
            return Err(crate::error::KimiCliError::Generic("pbcopy failed".into()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(crate::error::KimiCliError::Generic(
            "Clipboard access is only implemented for macOS in this port.".into(),
        ))
    }
}
