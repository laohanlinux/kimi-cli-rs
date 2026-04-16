use axum::{
    extract::{Multipart, Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared application state for the web server.
#[derive(Debug, Clone, Default)]
pub struct WebAppState {
    pub store: Arc<RwLock<crate::web::store::WebStore>>,
    pub runner: Arc<RwLock<crate::web::runner::WebRunner>>,
}

/// Web-facing session summary.
#[derive(Debug, Clone, Serialize)]
struct WebSession {
    id: String,
    title: String,
    work_dir: String,
    archived: bool,
    is_running: bool,
}

impl From<&crate::session::Session> for WebSession {
    fn from(s: &crate::session::Session) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            work_dir: s.work_dir.to_string_lossy().to_string(),
            archived: s.state.archived,
            is_running: false,
        }
    }
}

/// Builds the API router.
pub fn router() -> Router<WebAppState> {
    Router::new()
        .route("/healthz", get(health))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/:id",
            get(get_session).patch(update_session).delete(delete_session),
        )
        .route("/api/sessions/:id/fork", post(fork_session))
        .route("/api/sessions/:id/generate-title", post(generate_title))
        .route("/api/sessions/:id/git-diff", get(git_diff))
        .route("/api/sessions/:id/files", post(upload_file))
        .route("/api/sessions/:id/files/*path", get(get_session_file))
        .route("/api/sessions/:id/uploads/*path", get(get_upload_file))
        .route("/api/sessions/:id/stream", get(session_stream))
        .nest("/api/work-dirs", work_dirs_router())
}

fn work_dirs_router() -> Router<WebAppState> {
    Router::new()
        .route("/", get(list_work_dirs))
        .route("/startup", get(get_startup_dir))
}

// ------------------------------------------------------------------
// DTOs
// ------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ListSessionsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    q: Option<String>,
    archived: Option<bool>,
}

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Clone, Deserialize)]
struct CreateSessionRequest {
    work_dir: Option<String>,
    #[serde(default)]
    create_dir: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateSessionRequest {
    title: Option<String>,
    archived: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct ForkSessionRequest {
    turn_index: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GenerateTitleRequest {
    user_message: Option<String>,
    assistant_response: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GenerateTitleResponse {
    title: String,
}

#[derive(Debug, Clone, Serialize)]
struct GitFileDiff {
    path: String,
    additions: i64,
    deletions: i64,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct GitDiffStats {
    is_git_repo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_changes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_additions: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_deletions: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<GitFileDiff>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ------------------------------------------------------------------
// Handlers
// ------------------------------------------------------------------

/// Health check endpoint.
#[tracing::instrument(level = "debug")]
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// Lists sessions with optional pagination and filtering.
#[tracing::instrument(level = "debug", skip(state))]
async fn list_sessions(
    State(state): State<WebAppState>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = query.limit.clamp(1, 500) as usize;
    let offset = query.offset.max(0) as usize;
    let q = query.q.as_deref();
    let archived = query.archived;

    let store = state.store.read().await;
    let runner = state.runner.read().await;
    let mut sessions: Vec<WebSession> = store
        .list_paged(limit, offset, q, archived)
        .into_iter()
        .map(|s| WebSession::from(s))
        .collect();
    drop(store);
    for ws in &mut sessions {
        ws.is_running = runner.is_running(&ws.id).await;
    }

    Ok(Json(json!({ "sessions": sessions })))
}

/// Gets a single session by ID.
#[tracing::instrument(level = "debug", skip(state))]
async fn get_session(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> Result<Json<WebSession>, StatusCode> {
    let store = state.store.read().await;
    match store.get(&id) {
        Some(s) => Ok(Json(WebSession::from(s))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Creates a new session.
#[tracing::instrument(level = "debug", skip(state))]
async fn create_session(
    State(state): State<WebAppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<WebSession>, StatusCode> {
    let work_dir = match req.work_dir {
        Some(ref wd) => {
            let path = PathBuf::from(wd);
            if !path.exists() {
                if req.create_dir {
                    if let Err(e) = tokio::fs::create_dir_all(&path).await {
                        tracing::warn!("Failed to create work_dir {}: {}", wd, e);
                        return Err(StatusCode::FORBIDDEN);
                    }
                } else {
                    return Err(StatusCode::NOT_FOUND);
                }
            }
            path
        }
        None => dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
    };

    let session = crate::session::create(work_dir, None, None)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let ws = WebSession::from(&session);
    state.store.write().await.insert(session);
    Ok(Json(ws))
}

/// Updates a session (title / archive).
#[tracing::instrument(level = "debug", skip(state))]
async fn update_session(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<WebSession>, StatusCode> {
    let mut store = state.store.write().await;
    let session = store.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    let dir = session.dir();
    let mut st = crate::session_state::load_session_state(&dir);

    if let Some(title) = req.title {
        st.custom_title = Some(title);
        st.title_generated = true;
    }
    if let Some(archived) = req.archived {
        st.archived = archived;
        if archived {
            st.archived_at = Some(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64());
            st.auto_archive_exempt = false;
        } else {
            st.archived_at = None;
            st.auto_archive_exempt = true;
        }
    }

    if let Err(e) = crate::session_state::save_session_state(&st, &dir) {
        tracing::warn!("Failed to save session state for {}: {}", id, e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    session.state = st;
    session.refresh().await;
    let ws = WebSession::from(session as &crate::session::Session);
    Ok(Json(ws))
}

/// Deletes a session.
#[tracing::instrument(level = "debug", skip(state))]
async fn delete_session(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let session = {
        let mut store = state.store.write().await;
        store.remove(&id)
    };
    if let Some(s) = session {
        if let Err(e) = s.delete().await {
            tracing::warn!("Failed to delete session dir for {}: {}", id, e);
        }
    } else {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Forks a session.
#[tracing::instrument(level = "debug", skip(state))]
async fn fork_session(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    Json(_req): Json<ForkSessionRequest>,
) -> Result<Json<WebSession>, StatusCode> {
    let source = {
        let store = state.store.read().await;
        store.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?
    };

    let turn_index = _req.turn_index as usize;
    let new_session = crate::session_fork::fork(&source, Some(turn_index))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let ws = WebSession::from(&new_session);
    state.store.write().await.insert(new_session);
    Ok(Json(ws))
}

/// Generates a session title, optionally using the configured LLM.
#[tracing::instrument(level = "debug", skip(state))]
async fn generate_title(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    Json(req): Json<GenerateTitleRequest>,
) -> Result<Json<GenerateTitleResponse>, StatusCode> {
    let mut store = state.store.write().await;
    let session = store.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    let dir = session.dir();
    let mut st = crate::session_state::load_session_state(&dir);

    if st.title_generated {
        return Ok(Json(GenerateTitleResponse {
            title: st.custom_title.unwrap_or_else(|| "Untitled".into()),
        }));
    }

    let title = match generate_title_with_llm(&req).await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) | Err(_) => {
            let fallback = req
                .user_message
                .as_deref()
                .unwrap_or("Untitled")
                .trim()
                .to_string();
            if fallback.len() > 50 {
                format!("{}...", &fallback[..47])
            } else {
                fallback
            }
        }
    };

    st.custom_title = Some(title.clone());
    st.title_generated = true;
    let _ = crate::session_state::save_session_state(&st, &dir);
    session.state = st;
    session.refresh().await;

    Ok(Json(GenerateTitleResponse { title }))
}

/// Attempts to generate a concise session title using the configured LLM.
async fn generate_title_with_llm(req: &GenerateTitleRequest) -> crate::error::Result<String> {
    let config = crate::config::load_config(None)?;

    let (model, provider) = if !config.default_model.is_empty() {
        config
            .models
            .get(&config.default_model)
            .and_then(|m| config.providers.get(&m.provider).map(|p| (m.clone(), p.clone())))
    } else {
        None
    }
    .unwrap_or_else(|| {
        (
            crate::config::LlmModel {
                provider: "kimi".into(),
                model: "kimi-k2.5".into(),
                max_context_size: 128_000,
                capabilities: None,
            },
            crate::config::LlmProvider {
                r#type: "kimi".into(),
                base_url: "https://api.moonshot.cn".into(),
                api_key: secrecy::SecretString::new("".into()),
                env: None,
                custom_headers: None,
                oauth: None,
            },
        )
    });

    let llm = match crate::llm::create_llm(&provider, &model, None, None).await? {
        Some(l) => l,
        None => return Ok(String::new()),
    };

    let mut prompt = String::from(
        "Generate a short, concise title (maximum 50 characters) for a chat session. \
         Respond with ONLY the title text. No quotes, no explanations, no markdown.\n\n",
    );
    if let Some(ref user) = req.user_message {
        prompt.push_str("User message: ");
        prompt.push_str(user);
        prompt.push('\n');
    }
    if let Some(ref assistant) = req.assistant_response {
        prompt.push_str("Assistant response: ");
        prompt.push_str(assistant);
        prompt.push('\n');
    }

    let system_msg = crate::soul::message::Message {
        role: "system".into(),
        content: vec![crate::soul::message::ContentPart::Text {
            text: prompt,
        }],
        tool_calls: None,
        tool_call_id: None,
    };

    let reply = llm
        .chat(None, &[system_msg], None)
        .await?;

    let raw = reply.extract_text("").trim().to_string();
    let cleaned = raw
        .trim_matches('"')
        .trim_matches('\'')
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if cleaned.len() > 50 {
        Ok(format!("{}...", &cleaned[..47]))
    } else {
        Ok(cleaned)
    }
}

/// Returns git diff statistics for the session work directory.
#[tracing::instrument(level = "debug", skip(state))]
async fn git_diff(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
) -> Result<Json<GitDiffStats>, StatusCode> {
    let store = state.store.read().await;
    let session = store.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    let work_dir = session.work_dir.clone();

    let git_dir = work_dir.join(".git");
    if !git_dir.exists() {
        return Ok(Json(GitDiffStats {
            is_git_repo: false,
            has_changes: None,
            total_additions: None,
            total_deletions: None,
            files: None,
            error: None,
        }));
    }

    let result = tokio::process::Command::new("git")
        .args(["diff", "--numstat", "HEAD"])
        .current_dir(&work_dir)
        .output()
        .await;

    let mut files = Vec::new();
    let mut total_add = 0i64;
    let mut total_del = 0i64;

    match result {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    let add: i64 = parts[0].parse().unwrap_or(0);
                    let del: i64 = parts[1].parse().unwrap_or(0);
                    total_add += add;
                    total_del += del;
                    let status = if add > 0 && del == 0 {
                        "added"
                    } else if add == 0 && del > 0 {
                        "deleted"
                    } else {
                        "modified"
                    };
                    files.push(GitFileDiff {
                        path: parts[2].to_string(),
                        additions: add,
                        deletions: del,
                        status: status.to_string(),
                    });
                }
            }
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            return Ok(Json(GitDiffStats {
                is_git_repo: true,
                has_changes: Some(false),
                total_additions: None,
                total_deletions: None,
                files: None,
                error: Some(err),
            }));
        }
        Err(e) => {
            return Ok(Json(GitDiffStats {
                is_git_repo: true,
                has_changes: Some(false),
                total_additions: None,
                total_deletions: None,
                files: None,
                error: Some(e.to_string()),
            }));
        }
    }

    // Also list untracked files
    let untracked = tokio::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(&work_dir)
        .output()
        .await;

    if let Ok(out) = untracked {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if !line.is_empty() {
                    files.push(GitFileDiff {
                        path: line.to_string(),
                        additions: 0,
                        deletions: 0,
                        status: "added".into(),
                    });
                }
            }
        }
    }

    Ok(Json(GitDiffStats {
        is_git_repo: true,
        has_changes: Some(!files.is_empty()),
        total_additions: Some(total_add),
        total_deletions: Some(total_del),
        files: Some(files),
        error: None,
    }))
}

/// Uploads a file to a session.
#[tracing::instrument(level = "debug", skip(state, multipart))]
async fn upload_file(
    State(state): State<WebAppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let store = state.store.read().await;
    let session = store.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    let uploads_dir = session.dir().join("uploads");
    std::fs::create_dir_all(&uploads_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut saved = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let filename = field.file_name().unwrap_or("upload").to_string();
        let path = uploads_dir.join(&filename);
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        tokio::fs::write(&path, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        saved.push(json!({
            "filename": filename,
            "size": data.len(),
            "path": path.to_string_lossy().to_string(),
        }));
    }

    Ok(Json(json!({
        "uploaded": saved,
        "message": "File upload successful"
    })))
}

/// Gets a file or lists a directory from the session work directory.
#[tracing::instrument(level = "debug", skip(state))]
async fn get_session_file(
    State(state): State<WebAppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let store = state.store.read().await;
    let session = store.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    let work_dir = session.work_dir.clone();

    let target = work_dir.join(&path);
    let canonical = dunce::canonicalize(&target).unwrap_or(target.clone());
    if !canonical.starts_with(&work_dir) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if !canonical.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    if canonical.is_dir() {
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&canonical).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await.ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            if is_dir {
                entries.push(json!({"name": name, "type": "directory"}));
            } else {
                entries.push(json!({"name": name, "type": "file", "size": size}));
            }
        }
        entries.sort_by(|a, b| {
            let a_type = a.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let b_type = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            (a_type, a_name).cmp(&(b_type, b_name))
        });
        Ok((StatusCode::OK, Json(json!({"entries": entries}))).into_response())
    } else {
        let content = tokio::fs::read(&canonical).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok((
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            content,
        )
            .into_response())
    }
}

/// Gets an uploaded file from the session uploads directory.
#[tracing::instrument(level = "debug", skip(state))]
async fn get_upload_file(
    State(state): State<WebAppState>,
    Path((id, path)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let store = state.store.read().await;
    let session = store.get(&id).ok_or(StatusCode::NOT_FOUND)?;
    let uploads_dir = session.dir().join("uploads");
    let target = uploads_dir.join(&path);
    let canonical = dunce::canonicalize(&target).unwrap_or(target.clone());
    if !canonical.starts_with(&uploads_dir) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !canonical.is_file() {
        return Err(StatusCode::NOT_FOUND);
    }
    let content = tokio::fs::read(&canonical).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        content,
    )
        .into_response())
}

/// WebSocket stream for a session (stub).
#[tracing::instrument(level = "debug", skip(ws))]
async fn session_stream(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<WebAppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, id, state))
}

async fn handle_socket(
    mut socket: axum::extract::ws::WebSocket,
    id: String,
    state: WebAppState,
) {
    use axum::extract::ws::Message;

    let session = {
        let store = state.store.read().await;
        match store.get(&id).cloned() {
            Some(s) => s,
            None => {
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type": "Notification",
                            "payload": { "text": format!("Session {} not found", id) }
                        })
                        .to_string()
                        .into(),
                    ))
                    .await;
                return;
            }
        }
    };

    let worker = match state.runner.write().await.ensure_worker(&session).await {
        Ok(w) => w,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "Notification",
                        "payload": { "text": format!("Failed to start worker: {e}") }
                    })
                    .to_string()
                    .into(),
                ))
                .await;
            return;
        }
    };

    let input_tx = worker.input_tx.clone();
    let mut wire_rx = worker.wire_tx.subscribe();

    loop {
        tokio::select! {
            Ok(msg) = wire_rx.recv() => {
                let text = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(%e, "failed to serialize wire message for websocket");
                        continue;
                    }
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            Some(Ok(msg)) = socket.recv() => {
                match msg {
                    Message::Text(text) => {
                        let _ = input_tx.send(text);
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            else => break,
        }
    }

    state.runner.write().await.drop_worker(&id).await;
}

/// Lists known work directories.
#[tracing::instrument(level = "debug")]
async fn list_work_dirs() -> Json<serde_json::Value> {
    let metadata = crate::metadata::load_metadata();
    let dirs: Vec<String> = metadata
        .work_dirs
        .into_iter()
        .filter_map(|wd| {
            let path = PathBuf::from(&wd.path);
            if path.exists() {
                Some(path.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();
    Json(json!({ "work_dirs": dirs }))
}

/// Returns the startup directory.
#[tracing::instrument(level = "debug")]
async fn get_startup_dir() -> Json<serde_json::Value> {
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    Json(json!({ "startup_dir": cwd }))
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_session_from_session() {
        let s = crate::session::Session {
            id: "1".into(),
            work_dir: PathBuf::from("/tmp"),
            work_dir_meta: crate::metadata::WorkDirMeta {
                path: "/tmp".into(),
                kaos: "local".into(),
                last_session_id: None,
            },
            context_file: PathBuf::from("/tmp/context.jsonl"),
            wire_file: crate::wire::file::WireFile::new(PathBuf::from("/tmp/wire.jsonl")),
            state: crate::session_state::SessionState::default(),
            title: "Test".into(),
            updated_at: 0.0,
        };
        let ws = WebSession::from(&s);
        assert_eq!(ws.id, "1");
        assert_eq!(ws.title, "Test");
    }

    #[test]
    fn generate_title_response_serializes() {
        let r = GenerateTitleResponse {
            title: "hello".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("hello"));
    }
}
