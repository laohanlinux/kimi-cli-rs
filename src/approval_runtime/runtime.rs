use regex::Regex;
use std::sync::{Arc, Mutex};

/// Decision produced by the approval runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    Deny { reason: String },
    RequestUser,
}

/// Runtime for evaluating approval rules.
#[derive(Debug, Clone, Default)]
pub struct ApprovalRuntime {
    auto_approve_patterns: Arc<Mutex<Vec<String>>>,
    deny_patterns: Arc<Mutex<Vec<String>>>,
    _root_wire_hub: Arc<Mutex<Option<Arc<crate::wire::root_hub::RootWireHub>>>>,
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
        let mut rt = ApprovalRuntime::default();
        rt.add_auto_approve_pattern("ReadFile");
        assert_eq!(rt.evaluate("ReadFile", &serde_json::Value::Null), ApprovalDecision::Approve);
    }

    #[test]
    fn deny_takes_precedence() {
        let mut rt = ApprovalRuntime::default();
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
        let mut rt = ApprovalRuntime::default();
        rt.add_auto_approve_pattern("Read*");
        assert_eq!(rt.evaluate("ReadFile", &serde_json::Value::Null), ApprovalDecision::Approve);
        assert_eq!(
            rt.evaluate("WriteFile", &serde_json::Value::Null),
            ApprovalDecision::RequestUser
        );
    }
}
