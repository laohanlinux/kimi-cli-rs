/// D-Mail message for time-travel context reversion.
#[derive(Debug, Clone)]
pub struct DMail {
    pub message: String,
    pub checkpoint_id: usize,
}

/// Time-travel phone booth for reverting context to checkpoints.
#[derive(Debug, Clone, Default)]
pub struct DenwaRenji {
    pending_dmail: Option<DMail>,
    n_checkpoints: usize,
}

impl DenwaRenji {
    /// Sends a D-Mail to revert to a checkpoint.
    pub fn send_dmail(&mut self,
        dmail: DMail,
    ) -> crate::error::Result<()> {
        if self.pending_dmail.is_some() {
            return Err(crate::error::KimiCliError::Generic(
                "Only one D-Mail can be sent at a time".into(),
            ));
        }
        if dmail.checkpoint_id >= self.n_checkpoints {
            return Err(crate::error::KimiCliError::Generic(
                "There is no checkpoint with the given ID".into(),
            ));
        }
        self.pending_dmail = Some(dmail);
        Ok(())
    }

    /// Sets the number of known checkpoints.
    pub fn set_n_checkpoints(&mut self,
        n: usize,
    ) {
        self.n_checkpoints = n;
    }

    /// Fetches and clears any pending D-Mail.
    pub fn fetch_pending_dmail(&mut self,
    ) -> Option<DMail> {
        self.pending_dmail.take()
    }
}
