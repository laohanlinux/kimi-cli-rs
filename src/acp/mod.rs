use axum::{
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared ACP application state.
#[derive(Debug, Clone, Default)]
pub struct AcpState {
    pub sessions: Arc<RwLock<std::collections::HashMap<String, String>>>,
}

/// ACP (Agent Control Protocol) server.
#[derive(Debug, Clone, Default)]
pub struct AcpServer {
    pub port: u16,
}

impl AcpServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(&self) -> crate::error::Result<()> {
        let state = AcpState::default();
        let app = router().with_state(state);

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port))
            .await
            .map_err(|e| crate::error::KimiCliError::Io(e))?;
        tracing::info!("ACP server listening on port {}", self.port);
        axum::serve(listener, app)
            .await
            .map_err(|e| crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }
}

/// Builds the ACP API router.
pub fn router() -> Router<AcpState> {
    Router::new()
        .route("/healthz", get(health))
        .route("/rpc", post(rpc_handler))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    id: serde_json::Value,
}

async fn rpc_handler(
    Json(req): Json<RpcRequest>,
) -> Json<serde_json::Value> {
    match req.method.as_str() {
        "initialize" => Json(json!({
            "jsonrpc": "2.0",
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": { "name": "kimi-cli-rs-acp", "version": "0.1.0" }
            },
            "id": req.id
        })),
        "tools/list" => Json(json!({
            "jsonrpc": "2.0",
            "result": { "tools": [] },
            "id": req.id
        })),
        _ => Json(json!({
            "jsonrpc": "2.0",
            "error": { "code": -32601, "message": format!("Method not found: {}", req.method) },
            "id": req.id
        })),
    }
}
