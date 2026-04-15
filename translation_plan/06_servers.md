# Phase 6: Servers Translation Plan

## Objective
Replace Python FastAPI/Flask-based servers with `axum` Routers. Expose REST endpoints, WebSocket streams, health checks, and JSON-RPC compatibility layers.

## 6.1 Web Server (`src/web/`)

### `src/web/mod.rs`

**Strategy:** `WebServer` struct holds a port and builds/serves an `axum` application.

```rust
/// Axum web server for the agent.
#[derive(Debug, Clone, Default)]
pub struct WebServer {
    pub port: u16,
}

impl WebServer {
    pub fn new(port: u16) -> Self { Self { port } }

    pub async fn serve(&self) -> crate::error::Result<()> {
        let state = api::WebAppState::default();
        let app = api::router()
            .with_state(state.clone())
            .route("/", axum::routing::get(|| async { "Kimi CLI Web Server" }));

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port)).await?;
        tracing::info!("Web server listening on port {}", self.port);
        axum::serve(listener, app).await?;
        Ok(())
    }
}
```

### `src/web/api/mod.rs`

**Strategy:** Build a single `Router<WebAppState>` with handlers for session CRUD, git diff, file upload, and WebSocket streaming.

**State:**
```rust
#[derive(Debug, Clone, Default)]
pub struct WebAppState {
    pub store: Arc<RwLock<crate::web::store::WebStore>>,
    pub runner: Arc<RwLock<crate::web::runner::WebRunner>>,
}
```

**Routes:**
| Route | Methods | Description |
|-------|---------|-------------|
| `/healthz` | GET | Health check |
| `/api/sessions` | GET, POST | List / create sessions |
| `/api/sessions/:id` | GET, PATCH, DELETE | Read / update / delete session |
| `/api/sessions/:id/fork` | POST | Fork session at turn index |
| `/api/sessions/:id/generate-title` | POST | LLM-based title generation |
| `/api/sessions/:id/git-diff` | GET | Git diff stats for work dir |
| `/api/sessions/:id/files` | POST | Upload file(s) |
| `/api/sessions/:id/files/*path` | GET | Read session work-dir file |
| `/api/sessions/:id/uploads/*path` | GET | Read uploaded file |
| `/api/sessions/:id/stream` | GET (WebSocket) | Live session stream stub |
| `/api/work-dirs` | GET | List known work dirs |
| `/api/work-dirs/startup` | GET | Current startup directory |

**Title Generation (`generate_title`):**
- Loads config, resolves default model/provider, creates an ephemeral LLM client.
- Sends a short system prompt with the user message and assistant response.
- Cleans the reply (strips quotes, truncates to 50 chars).
- Falls back to a truncated user message if the LLM is unavailable.

**Git Diff (`git_diff`):**
- Checks for `.git` in the session work directory.
- Runs `git diff --numstat HEAD` and `git ls-files --others --exclude-standard`.
- Returns `GitDiffStats` with per-file additions/deletions and untracked files.

**WebSocket Stream (`session_stream`):**
- Upgrades the HTTP connection to a WebSocket.
- Currently a stub that sends a `SessionNotice` and echoes close events.

### `src/web/store/mod.rs`

**Strategy:** In-memory `HashMap` session store with pagination, filtering, and sorting.

```rust
pub struct WebStore {
    sessions: HashMap<String, crate::session::Session>,
}
```

Methods:
- `insert`, `get`, `get_mut`, `remove`
- `list_paged(limit, offset, query, archived)` — sorts by `updated_at` descending.

### `src/web/runner/mod.rs`

**Strategy:** Placeholder for future background task runner integration inside the web server.

```rust
#[derive(Debug, Clone, Default)]
pub struct WebRunner;
```

## 6.2 Vis Server (`src/vis/`)

**Strategy:** Lightweight diagnostics server for traces and metrics.

```rust
pub struct VisServer {
    pub port: u16,
}

impl VisServer {
    pub async fn serve(&self) -> crate::error::Result<()> {
        let app = router();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port)).await?;
        axum::serve(listener, app).await?;
        Ok(())
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/traces", get(list_traces))
        .route("/metrics", get(metrics))
}
```

**Endpoints:**
- `/healthz` → `{ "status": "ok" }`
- `/traces` → `{ "traces": [] }`
- `/metrics` → `{ "sessions_total": 0, "background_tasks_running": 0 }`

## 6.3 ACP Server (`src/acp/`)

**Strategy:** JSON-RPC over HTTP exposing an MCP-compatible initialization surface.

```rust
pub struct AcpServer {
    pub port: u16,
}

pub fn router() -> Router<AcpState> {
    Router::new()
        .route("/healthz", get(health))
        .route("/rpc", post(rpc_handler))
}
```

**RPC Methods:**
- `initialize` → Returns protocol version and server info.
- `tools/list` → Returns empty tools array.
- Any other method → `-32601 Method not found`.

## Tracing Strategy for Servers
- `WebServer::serve`, `VisServer::serve`, `AcpServer::serve` → `info` level.
- Every API handler gets `#[tracing::instrument(level = "debug", skip(state))]`.
- Record request latency and 4xx/5xx response counts.
