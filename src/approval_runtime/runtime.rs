use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::models::{
    ApprovalDecision, ApprovalRequestRecord, ApprovalRuntimeEvent, ApprovalSource,
};

/// Runtime for evaluating approval rules and managing approval requests.
pub struct ApprovalRuntime {
    auto_approve_patterns: Arc<Mutex<Vec<String>>>,
    deny_patterns: Arc<Mutex<Vec<String>>>,
    _root_wire_hub: Arc<Mutex<Option<Arc<crate::wire::root_hub::RootWireHub>>>>,
    requests: Arc<Mutex<HashMap<String, ApprovalRequestRecord>>>,
    waiters: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<(String, String)>>>>,
    subscribers: Arc<Mutex<HashMap<String, Box<dyn Fn(ApprovalRuntimeEvent) + Send + Sync>>>>,
}

impl std::fmt::Debug for ApprovalRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalRuntime").finish()
    }
}

impl Clone for ApprovalRuntime {
    fn clone(&self) -> Self {
        Self {
            auto_approve_patterns: Arc::clone(&self.auto_approve_patterns),
            deny_patterns: Arc::clone(&self.deny_patterns),
            _root_wire_hub: Arc::clone(&self._root_wire_hub),
            requests: Arc::clone(&self.requests),
            waiters: Arc::clone(&self.waiters),
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

impl Default for ApprovalRuntime {
    fn default() -> Self {
        Self {
            auto_approve_patterns: Arc::new(Mutex::new(Vec::new())),
            deny_patterns: Arc::new(Mutex::new(Vec::new())),
            _root_wire_hub: Arc::new(Mutex::new(None)),
            requests: Arc::new(Mutex::new(HashMap::new())),
            waiters: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ApprovalRuntime {
    /// Binds the runtime to the root wire hub.
    pub fn bind_root_wire_hub(&self, root_wire_hub: &Arc<crate::wire::root_hub::RootWireHub>) {
        let mut hub = self._root_wire_hub.lock().unwrap();
        *hub = Some(Arc::clone(root_wire_hub));
    }

    /// Adds an auto-approve pattern (supports `*` wildcards).
    pub fn add_auto_approve_pattern(&self, pattern: &str) {
        self.auto_approve_patterns.lock().unwrap().push(pattern.to_string());
    }

    /// Adds a deny pattern (supports `*` wildcards).
    pub fn add_deny_pattern(&self, pattern: &str) {
        self.deny_patterns.lock().unwrap().push(pattern.to_string());
    }

    /// Evaluates a tool call against configured rules.
    #[tracing::instrument(level = "debug", skip(self, _arguments))]
    pub fn evaluate(
        &self,
        tool_name: &str,
        _arguments: &serde_json::Value,
    ) -> ApprovalDecision {
        let deny_patterns = self.deny_patterns.lock().unwrap();
        for pattern in deny_patterns.iter() {
            if wildcard_match(pattern, tool_name) {
                return ApprovalDecision::Deny {
                    reason: format!("tool '{}' matched deny pattern '{}'", tool_name, pattern),
                };
            }
        }
        drop(deny_patterns);

        let auto_approve_patterns = self.auto_approve_patterns.lock().unwrap();
        for pattern in auto_approve_patterns.iter() {
            if wildcard_match(pattern, tool_name) {
                return ApprovalDecision::Approve;
            }
        }

        ApprovalDecision::RequestUser
    }

    /// Creates a new approval request and publishes it to the wire.
    pub fn create_request(
        &self,
        request_id: String,
        tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        display: Vec<serde_json::Value>,
        source: ApprovalSource,
    ) -> ApprovalRequestRecord {
        let request = ApprovalRequestRecord::new(
            request_id.clone(),
            tool_call_id,
            sender,
            action,
            description,
            display,
            source,
        );
        self.requests.lock().unwrap().insert(request_id.clone(), request.clone());
        self._publish_event(ApprovalRuntimeEvent {
            kind: "request_created".into(),
            request: request.clone(),
        });
        self._publish_wire_request(&request);
        request
    }

    /// Waits for a response to the given request.
    pub async fn wait_for_response(
        &self,
        request_id: &str,
        timeout_secs: u64,
    ) -> crate::error::Result<(String, String)> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let requests = self.requests.lock().unwrap();
            let request = requests.get(request_id).ok_or_else(|| {
                crate::error::KimiCliError::Generic(format!(
                    "Approval request not found: {}",
                    request_id
                ))
            })?;
            if request.status == "cancelled" {
                return Err(crate::error::KimiCliError::ApprovalCancelled);
            }
            if request.status == "resolved" {
                let response = request.response.clone().unwrap_or_default();
                let feedback = request.feedback.clone();
                return Ok((response, feedback));
            }
            self.waiters.lock().unwrap().insert(request_id.into(), tx);
        }

        match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(crate::error::KimiCliError::ApprovalCancelled),
            Err(_) => {
                tracing::warn!("Approval request {} timed out after {}s", request_id, timeout_secs);
                self._cancel_request(request_id, "approval timed out");
                Err(crate::error::KimiCliError::ApprovalCancelled)
            }
        }
    }

    /// Resolves a pending request with the given response.
    pub fn resolve(&self, request_id: &str, response: &str, feedback: &str) -> bool {
        let mut requests = self.requests.lock().unwrap();
        let request = match requests.get_mut(request_id) {
            Some(r) if r.status == "pending" => r,
            _ => return false,
        };
        request.status = "resolved".into();
        request.response = Some(response.into());
        request.feedback = feedback.into();
        request.resolved_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );
        let request_clone = request.clone();
        drop(requests);

        if let Some(tx) = self.waiters.lock().unwrap().remove(request_id) {
            let _ = tx.send((response.into(), feedback.into()));
        }
        self._publish_event(ApprovalRuntimeEvent {
            kind: "request_resolved".into(),
            request: request_clone.clone(),
        });
        self._publish_wire_response(request_id, response, feedback);
        true
    }

    fn _cancel_request(&self, request_id: &str, feedback: &str) {
        let mut requests = self.requests.lock().unwrap();
        let request = match requests.get_mut(request_id) {
            Some(r) if r.status == "pending" => r,
            _ => return,
        };
        request.status = "cancelled".into();
        request.response = Some("reject".into());
        request.feedback = feedback.into();
        request.resolved_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );
        let request_clone = request.clone();
        drop(requests);

        self.waiters.lock().unwrap().remove(request_id);
        self._publish_event(ApprovalRuntimeEvent {
            kind: "request_resolved".into(),
            request: request_clone.clone(),
        });
        self._publish_wire_response(request_id, "reject", feedback);
    }

    /// Cancels all pending requests matching the given source.
    pub fn cancel_by_source(&self, source_kind: &str, source_id: &str) -> usize {
        let request_ids: Vec<String> = {
            let requests = self.requests.lock().unwrap();
            requests
                .values()
                .filter(|r| r.status == "pending" && r.source.kind == source_kind && r.source.id == source_id)
                .map(|r| r.id.clone())
                .collect()
        };
        for id in &request_ids {
            self._cancel_request(id, "");
        }
        request_ids.len()
    }

    /// Lists all pending requests sorted by creation time.
    pub fn list_pending(&self) -> Vec<ApprovalRequestRecord> {
        let requests = self.requests.lock().unwrap();
        let mut pending: Vec<_> = requests
            .values()
            .filter(|r| r.status == "pending")
            .cloned()
            .collect();
        pending.sort_by(|a, b| a.created_at.partial_cmp(&b.created_at).unwrap_or(std::cmp::Ordering::Equal));
        pending
    }

    /// Gets a request by ID.
    pub fn get_request(&self, request_id: &str) -> Option<ApprovalRequestRecord> {
        self.requests.lock().unwrap().get(request_id).cloned()
    }

    /// Subscribes to approval runtime events.
    pub fn subscribe(
        &self,
        callback: Box<dyn Fn(ApprovalRuntimeEvent) + Send + Sync>,
    ) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        self.subscribers.lock().unwrap().insert(token.clone(), callback);
        token
    }

    /// Unsubscribes from approval runtime events.
    pub fn unsubscribe(&self, token: &str) {
        self.subscribers.lock().unwrap().remove(token);
    }

    fn _publish_event(&self, event: ApprovalRuntimeEvent) {
        let subscribers = self.subscribers.lock().unwrap();
        for callback in subscribers.values() {
            callback(event.clone());
        }
    }

    fn _publish_wire_request(&self, request: &ApprovalRequestRecord) {
        let hub = self._root_wire_hub.lock().unwrap().clone();
        if let Some(hub) = hub {
            let msg = crate::wire::types::WireMessage::ApprovalRequest {
                id: request.id.clone(),
                tool_call_id: request.tool_call_id.clone(),
                sender: request.sender.clone(),
                action: request.action.clone(),
                description: request.description.clone(),
                display: if request.display.is_empty() {
                    None
                } else {
                    Some(serde_json::json!(request.display))
                },
            };
            let _ = hub.publish_nowait(msg);
        }
    }

    fn _publish_wire_response(&self, request_id: &str, response: &str, feedback: &str) {
        let hub = self._root_wire_hub.lock().unwrap().clone();
        if let Some(hub) = hub {
            let msg = crate::wire::types::WireMessage::ApprovalResponse {
                request_id: request_id.into(),
                response: response.into(),
                feedback: if feedback.is_empty() { None } else { Some(feedback.into()) },
            };
            let _ = hub.publish_nowait(msg);
        }
    }
}

/// Simple glob-style wildcard matching.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == text;
    }
    let regex_str = regex::escape(pattern).replace(r"\*", ".*");
    match Regex::new(&format!("^{}$", regex_str)) {
        Ok(re) => re.is_match(text),
        Err(_) => pattern == text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_approve_exact_match() {
        let rt = ApprovalRuntime::default();
        rt.add_auto_approve_pattern("ReadFile");
        assert_eq!(rt.evaluate("ReadFile", &serde_json::Value::Null), ApprovalDecision::Approve);
    }

    #[test]
    fn deny_takes_precedence() {
        let rt = ApprovalRuntime::default();
        rt.add_auto_approve_pattern("*");
        rt.add_deny_pattern("Shell");
        assert_eq!(
            rt.evaluate("Shell", &serde_json::Value::Null),
            ApprovalDecision::Deny {
                reason: "tool 'Shell' matched deny pattern 'Shell'".into(),
            }
        );
    }

    #[test]
    fn wildcard_auto_approve() {
        let rt = ApprovalRuntime::default();
        rt.add_auto_approve_pattern("Read*");
        assert_eq!(rt.evaluate("ReadFile", &serde_json::Value::Null), ApprovalDecision::Approve);
        assert_eq!(
            rt.evaluate("WriteFile", &serde_json::Value::Null),
            ApprovalDecision::RequestUser
        );
    }
}
