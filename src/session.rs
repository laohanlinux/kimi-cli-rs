use std::path::PathBuf;

/// A single work-directory session.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub work_dir: PathBuf,
    pub work_dir_meta: crate::metadata::WorkDirMeta,
    pub context_file: PathBuf,
    pub wire_file: crate::wire::file::WireFile,
    pub state: crate::session_state::SessionState,
    pub title: String,
    pub updated_at: f64,
}

impl Session {
    /// Returns the session directory, creating it if necessary.
    pub fn dir(&self) -> PathBuf {
        let path = self.work_dir_meta.sessions_dir().join(&self.id);
        std::fs::create_dir_all(&path).ok();
        path
    }

    /// Returns the subagent instances directory.
    pub fn subagents_dir(&self) -> PathBuf {
        let path = self.dir().join("subagents");
        std::fs::create_dir_all(&path).ok();
        path
    }

    /// Checks whether the session has any real history or a custom title.
    pub fn is_empty(&self) -> bool {
        if self.state.custom_title.is_some() {
            return false;
        }
        if !self.wire_file.is_empty() {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&self.context_file) else {
            return true;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(role) = record.get("role").and_then(|v| v.as_str()) {
                    if !role.starts_with('_') {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Saves mutable state to disk after reloading externally-modified fields.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn save_state(&mut self) -> crate::error::Result<()> {
        let fresh = crate::session_state::load_session_state(&self.dir());
        self.state.custom_title = fresh.custom_title;
        self.state.title_generated = fresh.title_generated;
        self.state.title_generate_attempts = fresh.title_generate_attempts;
        self.state.archived = fresh.archived;
        self.state.archived_at = fresh.archived_at;
        self.state.auto_archive_exempt = fresh.auto_archive_exempt;
        crate::session_state::save_session_state(&self.state, &self.dir())
    }

    /// Deletes the session directory and all its contents.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn delete(&self) -> crate::error::Result<()> {
        let dir = self.dir();
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Refreshes the session title and updated_at from the wire file.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn refresh(&mut self) {
        self.title = "Untitled".into();
        self.updated_at = if self.context_file.exists() {
            match tokio::fs::metadata(&self.context_file).await {
                Ok(m) => m
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok().map(|d| d.as_secs_f64()))
                    .unwrap_or(0.0),
                Err(_) => 0.0,
            }
        } else {
            0.0
        };

        if self.state.custom_title.is_some() {
            self.title = self.state.custom_title.clone().unwrap();
            return;
        }

        // Derive title from first TurnBegin in wire file.
        // Simplified: full implementation should iterate WireFile records.
    }
}

/// Creates a new session for the given work directory.
#[tracing::instrument(level = "debug")]
pub async fn create(
    work_dir: PathBuf,
    session_id: Option<String>,
    context_file: Option<PathBuf>,
) -> crate::error::Result<Session> {
    let canonical = dunce::canonicalize(&work_dir).unwrap_or(work_dir);
    let mut metadata = crate::metadata::load_metadata();
    let session_id = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let canonical_str = canonical.to_string_lossy().to_string();

    if let Some(pos) = metadata.work_dirs.iter().position(|wd| wd.path == canonical_str) {
        metadata.work_dirs[pos].last_session_id = Some(session_id.clone());
    } else {
        metadata.work_dirs.push(crate::metadata::WorkDirMeta {
            path: canonical_str.clone(),
            kaos: "local".into(),
            last_session_id: Some(session_id.clone()),
        });
    }
    let work_dir_meta = metadata.work_dirs.iter().find(|wd| wd.path == canonical_str).unwrap().clone();

    let session_dir = work_dir_meta.sessions_dir().join(&session_id);
    std::fs::create_dir_all(&session_dir)?;

    let context_file = if let Some(cf) = context_file {
        if let Some(parent) = cf.parent() {
            std::fs::create_dir_all(parent)?;
        }
        cf
    } else {
        session_dir.join("context.jsonl")
    };

    if context_file.exists() {
        tokio::fs::remove_file(&context_file).await?;
    }
    tokio::fs::File::create(&context_file).await?;

    crate::metadata::save_metadata(&metadata)?;

    let wire_file = crate::wire::file::WireFile::new(session_dir.join("wire.jsonl"));
    let mut session = Session {
        id: session_id,
        work_dir: canonical,
        work_dir_meta,
        context_file,
        wire_file,
        state: crate::session_state::SessionState::default(),
        title: String::new(),
        updated_at: 0.0,
    };
    session.refresh().await;
    Ok(session)
}

/// Finds a session by work directory and session ID.
#[tracing::instrument(level = "debug")]
pub async fn find(work_dir: PathBuf, session_id: &str) -> Option<Session> {
    let canonical = dunce::canonicalize(&work_dir).unwrap_or(work_dir);
    let metadata = crate::metadata::load_metadata();
    let work_dir_meta = metadata.get_work_dir_meta(&canonical)?;

    let session_dir = work_dir_meta.sessions_dir().join(session_id);
    if !session_dir.is_dir() {
        return None;
    }
    let context_file = session_dir.join("context.jsonl");
    if !context_file.exists() {
        return None;
    }

    let wire_file = crate::wire::file::WireFile::new(session_dir.join("wire.jsonl"));
    let state = crate::session_state::load_session_state(&session_dir);
    let mut session = Session {
        id: session_id.into(),
        work_dir: canonical,
        work_dir_meta: work_dir_meta.clone(),
        context_file,
        wire_file,
        state,
        title: String::new(),
        updated_at: 0.0,
    };
    session.refresh().await;
    Some(session)
}

/// Lists all non-empty sessions for a work directory.
#[tracing::instrument(level = "debug")]
pub async fn list(work_dir: PathBuf) -> Vec<Session> {
    let canonical = dunce::canonicalize(&work_dir).unwrap_or(work_dir);
    let metadata = crate::metadata::load_metadata();
    let Some(work_dir_meta) = metadata.get_work_dir_meta(&canonical) else {
        return Vec::new();
    };

    let mut sessions = Vec::new();
    let Ok(entries) = std::fs::read_dir(work_dir_meta.sessions_dir()) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let session_id = entry.file_name().to_string_lossy().to_string();
        let session_dir = entry.path();
        if !session_dir.is_dir() {
            continue;
        }
        let context_file = session_dir.join("context.jsonl");
        if !context_file.exists() {
            continue;
        }
        let wire_file = crate::wire::file::WireFile::new(session_dir.join("wire.jsonl"));
        let state = crate::session_state::load_session_state(&session_dir);
        let mut session = Session {
            id: session_id,
            work_dir: canonical.clone(),
            work_dir_meta: work_dir_meta.clone(),
            context_file,
            wire_file,
            state,
            title: String::new(),
            updated_at: 0.0,
        };
        if session.is_empty() {
            continue;
        }
        session.refresh().await;
        sessions.push(session);
    }

    sessions.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
    sessions
}

/// Returns the most recent session for a work directory, if any.
#[tracing::instrument(level = "debug")]
pub async fn continue_(work_dir: PathBuf) -> Option<Session> {
    let canonical = dunce::canonicalize(&work_dir).unwrap_or_else(|_| work_dir.clone());
    let metadata = crate::metadata::load_metadata();
    let work_dir_meta = metadata.get_work_dir_meta(&canonical)?;
    let last_id = work_dir_meta.last_session_id.as_ref()?;
    find(work_dir, last_id).await
}
