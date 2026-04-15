# Phase 4: Wire Protocol, Background, Subagents, UI, Servers Translation Plan

## 4.1 Wire Protocol (`src/wire/`)

### `src/wire/types.rs`
Replace Python unions with Rust enums using `#[serde(tag = "type", rename_all = "snake_case")]`.

```rust
use serde::{Deserialize, Serialize};

/// All messages that can travel over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMessage {
    // Control events
    TurnBegin { user_input: String },
    StepBegin { step_no: usize },
    StepInterrupted,
    TurnEnd { stop_reason: String },
    CompactionBegin,
    CompactionEnd,
    StatusUpdate { snapshot: StatusSnapshot },
    Notification { text: String },
    PlanDisplay { content: String },
    BtwBegin,
    BtwEnd,
    SubagentEvent { agent_id: String, event: String },
    HookTriggered { hook_name: String },
    HookResolved { hook_name: String, duration_ms: u64 },
    McpLoadingBegin,
    McpLoadingEnd,

    // Content parts
    TextPart { text: String },
    ThinkPart { thought: String },
    ImageUrlPart { url: String },
    AudioUrlPart { url: String },
    VideoUrlPart { url: String },

    // Tooling
    ToolCall { tool_call_id: String, name: String, arguments: serde_json::Value },
    ToolCallPart { tool_call_id: String, index: usize, content: serde_json::Value },
    ToolResult { tool_call_id: String, result: crate::soul::message::ToolReturnValue },
    ApprovalResponse { request_id: String, response: String, feedback: Option<String> },

    // Requests (expecting response)
    ApprovalRequest {
        id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Option<serde_json::Value>,
    },
    QuestionRequest {
        id: String,
        items: Vec<QuestionItem>,
    },
    ToolCallRequest {
        id: String,
        tool_call_id: String,
        name: String,
        arguments: serde_json::Value,
    },
    HookRequest {
        id: String,
        hook_name: String,
        input_data: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub context_usage: f64,
    pub yolo_enabled: bool,
    pub plan_mode: bool,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItem {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}
```

### `src/wire/mod.rs` (Wire channel)
```rust
use tokio::sync::broadcast;

/// Single-producer, multi-consumer wire channel.
pub struct Wire {
    raw_tx: broadcast::Sender<WireMessage>,
    merged_tx: broadcast::Sender<WireMessage>,
}

impl Wire {
    pub fn new() -> Self {
        let (raw_tx, _) = broadcast::channel<WireMessage>(1024);
        let (merged_tx, _) = broadcast::channel<WireMessage>(1024);
        Self { raw_tx, merged_tx }
    }

    pub fn soul_side(&self) -> WireSoulSide {
        WireSoulSide {
            raw_tx: self.raw_tx.clone(),
            merged_tx: self.merged_tx.clone(),
        }
    }

    pub fn ui_side(&self) -> WireUISide {
        WireUISide {
            raw_rx: self.raw_tx.subscribe(),
            merged_rx: self.merged_tx.subscribe(),
        }
    }
}

pub struct WireSoulSide {
    raw_tx: broadcast::Sender<WireMessage>,
    merged_tx: broadcast::Sender<WireMessage>,
}

impl WireSoulSide {
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn send(&self, msg: WireMessage) {
        let _ = self.raw_tx.send(msg);
    }
}

pub struct WireUISide {
    raw_rx: broadcast::Receiver<WireMessage>,
    merged_rx: broadcast::Receiver<WireMessage>,
}

impl WireUISide {
    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn recv(&mut self) -> Option<WireMessage> {
        self.merged_rx.recv().await.ok()
    }
}
```

### `src/wire/root_hub.rs`
```rust
use tokio::sync::broadcast;

/// Session-level broadcast hub for out-of-turn messages.
pub struct RootWireHub {
    tx: broadcast::Sender<crate::wire::types::WireMessage>,
}

impl RootWireHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<crate::wire::types::WireMessage> {
        self.tx.subscribe()
    }

    pub fn publish(&self, msg: crate::wire::types::WireMessage) {
        let _ = self.tx.send(msg);
    }
}
```

### `src/wire/server.rs` (JSON-RPC bridge)
Use `tokio::io::{AsyncBufReadExt, AsyncWriteExt}` for stdio JSON-RPC.

```rust
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Bridges the internal Wire to a JSON-RPC over stdio client.
pub struct WireServer {
    soul: crate::soul::kimisoul::KimiSoul,
}

impl WireServer {
    pub fn new(soul: crate::soul::kimisoul::KimiSoul) -> Self {
        Self { soul }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(self) -> crate::Result<()> {
        let stdin = BufReader::new(io::stdin());
        let mut stdout = io::stdout();
        let mut lines = stdin.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(bytes = line.len(), "jsonrpc read line");
            let request: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(%e, "invalid jsonrpc line");
                    continue;
                }
            };
            // Dispatch to handler...
            let response = Self::dispatch(request).await?;
            let out = format!("{}\n", serde_json::to_string(&response)?);
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
        Ok(())
    }

    async fn dispatch(req: serde_json::Value) -> crate::Result<serde_json::Value> {
        // Route to initialize, prompt, steer, replay, etc.
        todo!("implement jsonrpc dispatch")
    }
}
```

## 4.2 Background Tasks (`src/background/`)

### `src/background/manager.rs`
```rust
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages the lifecycle of background bash and agent tasks.
pub struct BackgroundTaskManager {
    store: Arc<RwLock<crate::background::store::BackgroundTaskStore>>,
    max_running: usize,
}

impl BackgroundTaskManager {
    #[tracing::instrument(level = "debug")]
    pub async fn create_bash_task(&self,
        command: &str,
        timeout_secs: u64,
    ) -> crate::background::models::TaskView {
        let spec = crate::background::models::TaskSpec {
            command: command.into(),
            timeout_secs,
            ..Default::default()
        };
        // Persist, spawn subprocess, return view.
        todo!("implement create_bash_task")
    }

    #[tracing::instrument(level = "debug")]
    pub async fn kill_task(&self,
        task_id: &str,
    ) -> crate::Result<()> {
        todo!("implement kill_task")
    }
}
```

## 4.3 Subagents (`src/subagents/`)

### `src/subagents/registry.rs`
```rust
/// Registry of available subagent types.
#[derive(Debug, Default)]
pub struct LaborMarket {
    types: std::collections::HashMap<String, AgentTypeDefinition>,
}

impl LaborMarket {
    pub fn register_builtin_type(&mut self,
        name: &str,
        definition: AgentTypeDefinition,
    ) {
        self.types.insert(name.into(), definition);
    }

    pub fn list_types(&self) -> Vec<&AgentTypeDefinition> {
        self.types.values().collect()
    }
}

#[derive(Debug, Clone)]
pub struct AgentTypeDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
}
```

### `src/subagents/runner.rs`
```rust
/// Runs a foreground subagent to completion and returns its summary.
pub struct ForegroundSubagentRunner;

impl ForegroundSubagentRunner {
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn run(
        request: crate::subagents::models::AgentLaunchSpec,
    ) -> crate::Result<String> {
        let start = std::time::Instant::now();
        // Prepare instance, run soul, collect output.
        let elapsed = start.elapsed();
        tracing::info!(?elapsed, "subagent run completed");
        todo!("implement foreground subagent runner")
    }
}
```

## 4.4 UI Shell (`src/ui/shell/`)

### Strategy
Replace `prompt_toolkit` + `Rich Live` with `ratatui` + `crossterm`.

### `src/ui/shell/mod.rs`
```rust
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::{backend::CrosstermBackend, Terminal};

/// Interactive REPL shell.
pub struct Shell {
    soul: crate::soul::kimisoul::KimiSoul,
    welcome_info: Vec<WelcomeInfoItem>,
    prefill_text: Option<String>,
}

impl Shell {
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(&mut self,
        command: Option<&str>,
    ) -> crate::Result<bool> {
        let backend = CrosstermBackend::new(std::io::stderr());
        let mut terminal = Terminal::new(backend)?;

        // Main event loop.
        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                            tracing::info!("Ctrl-C pressed, cancelling");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(true)
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        // Render input panel, status bar, live view blocks.
    }
}
```

### `src/ui/print/mod.rs`
```rust
/// Non-interactive print mode for piping.
pub struct Print {
    soul: crate::soul::kimisoul::KimiSoul,
    final_only: bool,
}

impl Print {
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(&mut self, command: Option<&str>) -> crate::Result<i32> {
        let input = if let Some(cmd) = command {
            cmd.to_string()
        } else {
            let mut buf = String::new();
            tokio::io::AsyncReadExt::read_to_string(&mut tokio::io::stdin(), &mut buf
            ).await?;
            buf
        };
        // Run soul, print wire messages as text or JSON lines.
        todo!("implement print mode")
    }
}
```

## 4.5 Web Server (`src/web/`)

### Strategy
Replace FastAPI with `axum`.

### `src/web/app.rs`
```rust
use axum::{routing::get, Router};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;

/// Builds the FastAPI-equivalent web application.
pub fn create_app() -> Router {
    Router::new()
        .route("/api/sessions", get(crate::web::api::sessions::list_sessions))
        .route("/api/sessions/:id/stream", get(crate::web::api::sessions::session_stream))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .fallback(crate::web::api::spa_fallback)
}
```

### `src/web/api/sessions.rs`
```rust
use axum::extract::{Path, Query, WebSocketUpgrade};

/// WebSocket handler for live session streaming.
pub async fn session_stream(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, session_id))
}

#[tracing::instrument(level = "info", skip(socket))]
async fn handle_socket(mut socket: axum::extract::ws::WebSocket, session_id: String) {
    // Load session, get or create worker, replay history, bridge messages.
    todo!("implement websocket bridge")
}
```

## 4.6 Vis Server (`src/vis/`)
```rust
use axum::{routing::get, Router};

pub fn create_app() -> Router {
    Router::new()
        .route("/api/sessions", get(crate::vis::api::sessions::list_sessions))
        .route("/api/sessions/:id/wire", get(crate::vis::api::sessions::get_wire_events))
        .fallback(crate::vis::api::spa_fallback)
}
```

## 4.7 ACP Server (`src/acp/`)
```rust
/// ACP server exposing Kimi as an MCP-compatible agent.
pub struct AcpServer;

impl AcpServer {
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(self) -> crate::Result<()> {
        // Accept stdio JSON-RPC, translate to internal wire.
        todo!("implement acp server")
    }
}
```

## Tracing Strategy for Wire / Servers / UI
- `WireServer::serve` → `info` span with total connection duration.
- `handle_socket` → `info` span with session_id.
- Every HTTP handler gets `#[tracing::instrument]`.
- `WireSoulSide::send` / `WireUISide::recv` → `trace` level for high-volume events.
- Background task spawn/kill → `info` with task_id.
- Subagent run → `info` span covering full lifetime.
