use std::path::PathBuf;

std::thread_local! {
    static SHARE_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
}

/// Overrides the share directory for the current thread.
#[doc(hidden)]
pub fn set_test_share_dir(path: PathBuf) {
    SHARE_DIR_OVERRIDE.with(|p| *p.borrow_mut() = Some(path));
}

/// Clears the thread-local share directory override.
#[doc(hidden)]
pub fn clear_test_share_dir() {
    SHARE_DIR_OVERRIDE.with(|p| *p.borrow_mut() = None);
}

/// Returns the Kimi share directory, defaulting to `~/.kimi`.
/// Creates the directory if it does not exist.
#[tracing::instrument]
pub fn get_share_dir() -> crate::error::Result<PathBuf> {
    if let Some(path) = SHARE_DIR_OVERRIDE.with(|p| p.borrow().clone()) {
        std::fs::create_dir_all(&path)?;
        return Ok(path);
    }
    let path = std::env::var("KIMI_SHARE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .expect("home directory should be available")
                .join(".kimi")
        });
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_share_dir_respects_env() {
        let tmp = std::env::temp_dir().join(format!("kimi-share-{}", uuid::Uuid::new_v4()));
        set_test_share_dir(tmp.clone());
        let dir = get_share_dir().unwrap();
        assert_eq!(dir, tmp);
        assert!(tmp.exists());
        clear_test_share_dir();
        std::fs::remove_dir_all(&tmp).ok();
    }
}
