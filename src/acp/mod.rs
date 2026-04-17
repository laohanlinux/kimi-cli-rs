use axum::{
    Router,
    extract::State,
    response::Json,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared ACP application state.
#[derive(Clone)]
pub struct AcpState {
    pub soul: Arc<Mutex<crate::soul::kimisoul::KimiSoul>>,
    pub runtime: crate::soul::agent::Runtime,
}

impl std::fmt::Debug for AcpState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpState")
            .field("runtime", &self.runtime)
            .finish_non_exhaustive()
    }
}

/// ACP (Agent Control Protocol) server.
#[derive(Debug, Clone)]
pub struct AcpServer {
    pub port: u16,
    state: AcpState,
}

impl AcpServer {
    pub fn new(
        port: u16,
        soul: crate::soul::kimisoul::KimiSoul,
        runtime: crate::soul::agent::Runtime,
    ) -> Self {
        Self {
            port,
            state: AcpState {
                soul: Arc::new(Mutex::new(soul)),
                runtime,
            },
        }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(&self) -> crate::error::Result<()> {
        let app = router().with_state(self.state.clone());

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port))
            .await
            .map_err(|e| crate::error::KimiCliError::Io(e))?;
        tracing::info!("ACP server listening on port {}", self.port);
        axum::serve(listener, app).await.map_err(|e| {
            crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        Ok(())
    }
}

/// Builds the ACP API router.
pub fn router() -> Router<AcpState> {
    Router::new()
        .route("/healthz", get(health))
        .route("/rpc", post(rpc_handler))
        .route("/replay", get(handle_replay))
        .route("/history", get(handle_history))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

async fn rpc_handler(State(state): State<AcpState>, Json(req): Json<RpcRequest>) -> Json<Value> {
    let result = match req.method.as_str() {
        "initialize" => handle_initialize(&req).await,
        "tools/list" => handle_tools_list(&state, &req).await,
        "tools/call" => handle_tools_call(&state, &req).await,
        _ => jsonrpc_error(-32601, &format!("Method not found: {}", req.method), req.id),
    };
    Json(result)
}

async fn handle_initialize(req: &RpcRequest) -> Value {
    jsonrpc_success(
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "kimi-cli-rs-acp",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
        req.id.clone(),
    )
}

async fn handle_tools_list(state: &AcpState, req: &RpcRequest) -> Value {
    let soul = state.soul.lock().await;
    let tools = soul.agent().toolset.tools().await;
    let tool_items: Vec<Value> = tools
        .values()
        .map(|t| {
            json!({
                "name": t.name(),
                "description": t.description(),
                "inputSchema": t.parameters_schema(),
            })
        })
        .collect();
    jsonrpc_success(json!({ "tools": tool_items }), req.id.clone())
}

async fn handle_tools_call(state: &AcpState, req: &RpcRequest) -> Value {
    let name = req
        .params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = req.params.get("arguments").cloned().unwrap_or_default();

    if name.is_empty() {
        return jsonrpc_error(-32602, "Missing tool name", req.id.clone());
    }

    let soul = state.soul.lock().await;
    let tools = soul.agent().toolset.tools().await;
    let Some(tool) = tools.get(name) else {
        return jsonrpc_error(-32602, &format!("Tool not found: {}", name), req.id.clone());
    };

    let result = tool.call(arguments, &state.runtime).await;
    let (content, is_error) = match result {
        crate::soul::message::ToolReturnValue::Ok { output, .. } => {
            (vec![json!({ "type": "text", "text": output })], false)
        }
        crate::soul::message::ToolReturnValue::Error { error } => {
            (vec![json!({ "type": "text", "text": error })], true)
        }
        crate::soul::message::ToolReturnValue::Parts { parts } => {
            let content: Vec<Value> = parts
                .into_iter()
                .map(|p| match p {
                    crate::soul::message::ContentPart::Text { text } => {
                        json!({ "type": "text", "text": text })
                    }
                    crate::soul::message::ContentPart::ImageUrl { url } => {
                        json!({ "type": "image", "data": url })
                    }
                    crate::soul::message::ContentPart::AudioUrl { url } => {
                        json!({ "type": "audio", "data": url })
                    }
                    crate::soul::message::ContentPart::VideoUrl { url } => {
                        json!({ "type": "video", "data": url })
                    }
                    crate::soul::message::ContentPart::Think { thought } => {
                        json!({ "type": "text", "text": format!("[think] {}", thought) })
                    }
                })
                .collect();
            (content, false)
        }
    };

    jsonrpc_success(
        json!({
            "content": content,
            "isError": is_error,
        }),
        req.id.clone(),
    )
}

async fn handle_replay(State(state): State<AcpState>) -> Json<Value> {
    let history: Vec<Value> = state
        .runtime
        .session
        .wire_file
        .records()
        .into_iter()
        .map(|record| json!({ "event": record }))
        .collect();
    Json(json!({ "session_id": state.runtime.session.id, "events": history }))
}

async fn handle_history(State(state): State<AcpState>) -> Json<Value> {
    let history: Vec<Value> = state
        .runtime
        .session
        .wire_file
        .records()
        .into_iter()
        .map(|record| json!({ "event": record }))
        .collect();
    Json(json!({ "session_id": state.runtime.session.id, "events": history }))
}

fn jsonrpc_success(result: Value, id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": id
    })
}

fn jsonrpc_error(code: i32, message: &str, id: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "error": { "code": code, "message": message },
        "id": id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_success_format() {
        let v = jsonrpc_success(json!({"tools": []}), Some(json!(1)));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
    }

    #[test]
    fn jsonrpc_error_format() {
        let v = jsonrpc_error(-32601, "not found", Some(json!(2)));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["error"]["code"], -32601);
    }
}
