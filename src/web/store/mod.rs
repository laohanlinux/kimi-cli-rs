use std::collections::HashMap;

/// In-memory session store for the web server.
#[derive(Debug, Clone, Default)]
pub struct WebStore {
    sessions: HashMap<String, crate::session::Session>,
}

impl WebStore {
    pub fn insert(&mut self, session: crate::session::Session) {
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn get(&self, id: &str) -> Option<&crate::session::Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut crate::session::Session> {
        self.sessions.get_mut(id)
    }

    pub fn list(&self) -> Vec<&crate::session::Session> {
        self.sessions.values().collect()
    }

    pub fn list_paged(
        &self,
        limit: usize,
        offset: usize,
        query: Option<&str>,
        archived: Option<bool>,
    ) -> Vec<&crate::session::Session> {
        let mut out: Vec<_> = self.sessions.values().collect();
        if let Some(q) = query {
            let q = q.to_lowercase();
            out.retain(|s| {
                s.title.to_lowercase().contains(&q)
                    || s.work_dir.to_string_lossy().to_lowercase().contains(&q)
            });
        }
        if let Some(a) = archived {
            out.retain(|s| s.state.archived == a);
        }
        out.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        out.into_iter().skip(offset).take(limit).collect()
    }

    pub fn remove(&mut self, id: &str) -> Option<crate::session::Session> {
        self.sessions.remove(id)
    }
}
