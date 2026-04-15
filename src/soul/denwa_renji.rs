/// D-Mail message for time-travel context reversion.
#[derive(Debug, Clone)]
pub struct DMail {
    pub message: String,
    pub checkpoint_id: usize,
}

/// Time-travel phone booth for reverting context to checkpoints.
#[derive(Debug, Default)]
pub struct DenwaRenji {
    pending_dmail: std::sync::Mutex<Option<DMail>>,
    n_checkpoints: std::sync::Mutex<usize>,
}

impl Clone for DenwaRenji {
    fn clone(&self) -> Self {
        Self {
            pending_dmail: std::sync::Mutex::new(self.pending_dmail.lock().unwrap_or_else(|e| e.into_inner()).clone()),
            n_checkpoints: std::sync::Mutex::new(*self.n_checkpoints.lock().unwrap_or_else(|e| e.into_inner())),
        }
    }
}

impl DenwaRenji {
    /// Sends a D-Mail to revert to a checkpoint.
    pub fn send_dmail(&self, dmail: DMail) -> crate::error::Result<()> {
        let mut pending = self.pending_dmail.lock().unwrap_or_else(|e| e.into_inner());
        if pending.is_some() {
            return Err(crate::error::KimiCliError::Generic(
                "Only one D-Mail can be sent at a time".into(),
            ));
        }
        let n = *self.n_checkpoints.lock().unwrap_or_else(|e| e.into_inner());
        if dmail.checkpoint_id >= n {
            return Err(crate::error::KimiCliError::Generic(
                "There is no checkpoint with the given ID".into(),
            ));
        }
        *pending = Some(dmail);
        Ok(())
    }

    /// Sets the number of known checkpoints.
    pub fn set_n_checkpoints(&self, n: usize) {
        *self.n_checkpoints.lock().unwrap_or_else(|e| e.into_inner()) = n;
    }

    /// Fetches and clears any pending D-Mail.
    pub fn fetch_pending_dmail(&self) -> Option<DMail> {
        self.pending_dmail.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
