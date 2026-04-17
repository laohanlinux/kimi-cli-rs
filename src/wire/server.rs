use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};



/// Bridges the internal Wire to a JSON-RPC over stdio client.
pub struct WireServer {
    soul: Arc<Mutex<crate::soul::kimisoul::KimiSoul>>,
    runtime: crate::soul::agent::Runtime,
    cancel_tx: Arc<Mutex<tokio::sync::watch::Sender<bool>>>,
    writer_tx: mpsc::UnboundedSender<serde_json::Value>,
    pending_requests: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<crate::wire::types::WireMessage>>>>,
}

impl WireServer {
    pub fn new(
        soul: crate::soul::kimisoul::KimiSoul,
        runtime: crate::soul::agent::Runtime,
    ) -> Self {
        let (cancel_tx, _) = tokio::sync::watch::channel(false);
        let (writer_tx, _) = mpsc::unbounded_channel();
        Self {
            soul: Arc::new(Mutex::new(soul)),
            runtime,
            cancel_tx: Arc::new(Mutex::new(cancel_tx)),
            writer_tx,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(mut self) -> crate::error::Result<()> {
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<serde_json::Value>();
        self.writer_tx = writer_tx.clone();
        let pending_requests = self.pending_requests.clone();

        // Writer task: serializes JSON-RPC messages to stdout.
        let writer_handle = tokio::spawn(async move {
            let mut stdout = io::stdout();
            while let Some(msg) = writer_rx.recv().await {
                let line = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(%e, "failed to serialize jsonrpc message");
                        continue;
                    }
                };
                if stdout.write_all(format!("{}\n", line).as_bytes()).await.is_err() {
                    break;
                }
                if stdout.flush().await.is_err() {
                    break;
                }
            }
        });

        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(bytes = line.len(), "jsonrpc read line");
            let value: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(%e, "invalid jsonrpc line");
                    continue;
                }
            };

            // Responses have an id, no method, and contain result or error.
            let is_response = value.get("id").is_some()
                && value.get("method").is_none()
                && (value.get("result").is_some() || value.get("error").is_some());

            if is_response {
                Self::route_client_response(value, &pending_requests).await;
                continue;
            }

            let req: crate::wire::jsonrpc::JsonRpcRequest = match serde_json::from_value(value) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(%e, "invalid jsonrpc request structure");
                    continue;
                }
            };

            let response = match req.method.as_str() {
                "initialize" => Self::handle_initialize(&req),
                "prompt" => self.handle_prompt(&req).await,
                "steer" => self.handle_steer(&req).await,
                "cancel" => self.handle_cancel(&req).await,
                "replay" => self.handle_replay(&req).await,
                "set_plan_mode" => self.handle_set_plan_mode(&req).await,
                _ => Ok(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", req.method)
                    }
                })),
            };

            match response {
                Ok(resp) => {
                    if writer_tx.send(resp).is_err() {
                        tracing::warn!("writer channel closed");
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(%e, "jsonrpc handler error");
                    let _ = writer_tx.send(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "error": { "code": -32603, "message": e.to_string() }
                    }));
                }
            }
        }

        drop(writer_tx);
        let _ = writer_handle.await;
        Ok(())
    }

    fn handle_initialize(req: &crate::wire::jsonrpc::JsonRpcRequest) -> crate::error::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": { "name": "kimi-cli-rs-wire", "version": env!("CARGO_PKG_VERSION") }
            }
        }))
    }

    async fn handle_prompt(&self, req: &crate::wire::jsonrpc::JsonRpcRequest) -> crate::error::Result<serde_json::Value> {
        let text = req
            .params
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input = vec![crate::soul::message::ContentPart::Text { text: text.to_string() }];

        let soul = self.soul.clone();
        let runtime = self.runtime.clone();
        let writer_tx = self.writer_tx.clone();
        let pending = self.pending_requests.clone();
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        *self.cancel_tx.lock().await = cancel_tx;

        tokio::spawn(async move {
            let mut soul = soul.lock().await;
            let wire = crate::wire::Wire::default();
            let wire_clone = wire.clone();

            let writer_tx_ui = writer_tx.clone();
            let ui_loop = move |wire: crate::wire::Wire| -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
                Box::pin(async move {
                    let mut ui_side = wire.ui_side();
                    while let Some(msg) = ui_side.recv().await {
                        Self::forward_wire_message(
                            msg,
                            &writer_tx_ui,
                            &pending,
                            &wire_clone,
                        )
                        .await;
                    }
                })
            };

            let result =
                crate::soul::run_soul(&mut *soul, input, ui_loop, cancel_rx, &runtime).await;

            let summary = match result {
                Ok(outcome) => serde_json::json!({ "stop_reason": format!("{:?}", outcome.stop_reason) }),
                Err(e) => serde_json::json!({ "error": e.to_string() }),
            };
            let _ = writer_tx.send(serde_json::json!({
                "jsonrpc": "2.0",
                "method": "turn_end",
                "params": summary
            }));
        });

        Ok(serde_json::json!({ "jsonrpc": "2.0", "id": req.id, "result": { "accepted": true } }))
    }

    async fn handle_steer(&self, req: &crate::wire::jsonrpc::JsonRpcRequest) -> crate::error::Result<serde_json::Value> {
        let text = req
            .params
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.soul.lock().await.steer(text);
        Ok(serde_json::json!({ "jsonrpc": "2.0", "id": req.id, "result": { "accepted": true } }))
    }

    async fn handle_cancel(&self, req: &crate::wire::jsonrpc::JsonRpcRequest) -> crate::error::Result<serde_json::Value> {
        self.cancel_tx.lock().await.send(true).ok();
        Ok(serde_json::json!({ "jsonrpc": "2.0", "id": req.id, "result": { "cancelled": true } }))
    }

    async fn handle_replay(&self, req: &crate::wire::jsonrpc::JsonRpcRequest) -> crate::error::Result<serde_json::Value> {
        let session = &self.runtime.session;
        let records = session.wire_file.records();
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let records: Vec<_> = if let Some(limit) = limit {
            records.into_iter().rev().take(limit).rev().collect()
        } else {
            records
        };

        for record in records {
            let event = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "wire_event",
                "params": record
            });
            if self.writer_tx.send(event).is_err() {
                break;
            }
        }

        Ok(serde_json::json!({ "jsonrpc": "2.0", "id": req.id, "result": { "replayed": true } }))
    }

    async fn handle_set_plan_mode(
        &self,
        req: &crate::wire::jsonrpc::JsonRpcRequest,
    ) -> crate::error::Result<serde_json::Value> {
        let enabled = req.params.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        {
            let mut soul = self.soul.lock().await;
            soul.plan_mode = enabled;
        }

        let session_dir = self.runtime.session.dir();
        let mut state = crate::session_state::load_session_state(&session_dir);
        state.plan_mode = enabled;
        if enabled {
            state.plan_session_id = Some(self.runtime.session.id.clone());
        } else {
            state.plan_session_id = None;
        }
        let _ = crate::session_state::save_session_state(&state, &session_dir);

        Ok(serde_json::json!({ "jsonrpc": "2.0", "id": req.id, "result": { "plan_mode": enabled } }))
    }

    async fn forward_wire_message(
        msg: crate::wire::types::WireMessage,
        writer_tx: &mpsc::UnboundedSender<serde_json::Value>,
        pending: &Mutex<HashMap<String, mpsc::UnboundedSender<crate::wire::types::WireMessage>>>,
        wire: &crate::wire::Wire,
    ) {
        let (jsonrpc_id, method) = match &msg {
            crate::wire::types::WireMessage::ApprovalRequest { id, .. } => {
                (format!("approval-{id}"), "approval_request")
            }
            crate::wire::types::WireMessage::QuestionRequest { id, .. } => {
                (format!("question-{id}"), "question_request")
            }
            crate::wire::types::WireMessage::ToolCallRequest { id, .. } => {
                (format!("toolcall-{id}"), "tool_call_request")
            }
            crate::wire::types::WireMessage::HookRequest { id, .. } => {
                (format!("hook-{id}"), "hook_request")
            }
            _ => {
                let event = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "wire_event",
                    "params": msg
                });
                let _ = writer_tx.send(event);
                return;
            }
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        pending.lock().await.insert(jsonrpc_id.clone(), tx);

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": jsonrpc_id,
            "method": method,
            "params": msg
        });
        let _ = writer_tx.send(req);

        let ui_side = wire.ui_side();
        tokio::spawn(async move {
            if let Some(resp) = rx.recv().await {
                let _ = ui_side.send_response(resp).await;
            }
        });
    }

    async fn route_client_response(
        value: serde_json::Value,
        pending: &Mutex<HashMap<String, mpsc::UnboundedSender<crate::wire::types::WireMessage>>>,
    ) {
        let id = match value.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return,
        };

        let tx = pending.lock().await.remove(&id);
        let Some(tx) = tx else {
            tracing::warn!(%id, "received jsonrpc response for unknown request");
            return;
        };

        // Determine request type from id prefix.
        let wire_msg = if id.starts_with("approval-") {
            let request_id = id.strip_prefix("approval-").unwrap_or(&id).to_string();
            let response = value
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("reject")
                .to_string();
            let feedback = value
                .get("error")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
                .map(String::from);
            crate::wire::types::WireMessage::ApprovalResponse {
                request_id,
                response,
                feedback,
            }
        } else if id.starts_with("question-") {
            let request_id = id.strip_prefix("question-").unwrap_or(&id).to_string();
            let answers = value
                .get("result")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            crate::wire::types::WireMessage::QuestionResponse {
                request_id,
                answers,
            }
        } else if id.starts_with("toolcall-") {
            let request_id = id.strip_prefix("toolcall-").unwrap_or(&id).to_string();
            let result = value.get("result").cloned().unwrap_or_default();
            crate::wire::types::WireMessage::ToolResult {
                tool_call_id: request_id,
                result: crate::soul::message::ToolReturnValue::Ok {
                    output: result.to_string(),
                    message: None,
                },
            }
        } else if id.starts_with("hook-") {
            let request_id = id.strip_prefix("hook-").unwrap_or(&id).to_string();
            let result = value.get("result").cloned().unwrap_or_default();
            crate::wire::types::WireMessage::ToolResult {
                tool_call_id: request_id,
                result: crate::soul::message::ToolReturnValue::Ok {
                    output: result.to_string(),
                    message: None,
                },
            }
        } else {
            tracing::warn!(%id, "unknown request prefix in jsonrpc response");
            return;
        };

        let _ = tx.send(wire_msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_response_format() {
        let req = crate::wire::jsonrpc::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: serde_json::Value::Null,
        };
        let resp = WireServer::handle_initialize(&req).unwrap();
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert!(resp["result"]["serverInfo"]["name"].as_str().unwrap().contains("kimi"));
    }

    #[tokio::test]
    async fn route_client_response_approval() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, mut rx) = mpsc::unbounded_channel();
        pending.lock().await.insert("approval-req-1".into(), tx);

        let value = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "approval-req-1",
            "result": "approve"
        });
        WireServer::route_client_response(value, &pending).await;

        let msg = rx.recv().await.unwrap();
        match msg {
            crate::wire::types::WireMessage::ApprovalResponse { request_id, response, .. } => {
                assert_eq!(request_id, "req-1");
                assert_eq!(response, "approve");
            }
            _ => panic!("expected ApprovalResponse"),
        }
    }
}
