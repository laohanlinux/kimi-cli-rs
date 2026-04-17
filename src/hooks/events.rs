use serde_json::{Map, Value};

/// Builds the base payload common to all hook events.
fn base(event: &str, session_id: &str, cwd: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("hook_event_name".into(), Value::String(event.into()));
    map.insert("session_id".into(), Value::String(session_id.into()));
    map.insert("cwd".into(), Value::String(cwd.into()));
    map
}

/// PreToolUse hook payload.
pub fn pre_tool_use(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &Map<String, Value>,
    tool_call_id: &str,
) -> Map<String, Value> {
    let mut payload = base("PreToolUse", session_id, cwd);
    payload.insert("tool_name".into(), Value::String(tool_name.into()));
    payload.insert("tool_input".into(), Value::Object(tool_input.clone()));
    payload.insert("tool_call_id".into(), Value::String(tool_call_id.into()));
    payload
}

/// PostToolUse hook payload.
pub fn post_tool_use(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &Map<String, Value>,
    tool_output: &str,
    tool_call_id: &str,
) -> Map<String, Value> {
    let mut payload = base("PostToolUse", session_id, cwd);
    payload.insert("tool_name".into(), Value::String(tool_name.into()));
    payload.insert("tool_input".into(), Value::Object(tool_input.clone()));
    payload.insert("tool_output".into(), Value::String(tool_output.into()));
    payload.insert("tool_call_id".into(), Value::String(tool_call_id.into()));
    payload
}

/// PostToolUseFailure hook payload.
pub fn post_tool_use_failure(
    session_id: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &Map<String, Value>,
    error: &str,
    tool_call_id: &str,
) -> Map<String, Value> {
    let mut payload = base("PostToolUseFailure", session_id, cwd);
    payload.insert("tool_name".into(), Value::String(tool_name.into()));
    payload.insert("tool_input".into(), Value::Object(tool_input.clone()));
    payload.insert("error".into(), Value::String(error.into()));
    payload.insert("tool_call_id".into(), Value::String(tool_call_id.into()));
    payload
}

/// UserPromptSubmit hook payload.
pub fn user_prompt_submit(session_id: &str, cwd: &str, prompt: &str) -> Map<String, Value> {
    let mut payload = base("UserPromptSubmit", session_id, cwd);
    payload.insert("prompt".into(), Value::String(prompt.into()));
    payload
}

/// Stop hook payload.
pub fn stop(session_id: &str, cwd: &str, stop_hook_active: bool) -> Map<String, Value> {
    let mut payload = base("Stop", session_id, cwd);
    payload.insert("stop_hook_active".into(), Value::Bool(stop_hook_active));
    payload
}

/// StopFailure hook payload.
pub fn stop_failure(
    session_id: &str,
    cwd: &str,
    error_type: &str,
    error_message: &str,
) -> Map<String, Value> {
    let mut payload = base("StopFailure", session_id, cwd);
    payload.insert("error_type".into(), Value::String(error_type.into()));
    payload.insert("error_message".into(), Value::String(error_message.into()));
    payload
}

/// SessionStart hook payload.
pub fn session_start(session_id: &str, cwd: &str, source: &str) -> Map<String, Value> {
    let mut payload = base("SessionStart", session_id, cwd);
    payload.insert("source".into(), Value::String(source.into()));
    payload
}

/// SessionEnd hook payload.
pub fn session_end(session_id: &str, cwd: &str, reason: &str) -> Map<String, Value> {
    let mut payload = base("SessionEnd", session_id, cwd);
    payload.insert("reason".into(), Value::String(reason.into()));
    payload
}

/// SubagentStart hook payload.
pub fn subagent_start(
    session_id: &str,
    cwd: &str,
    agent_name: &str,
    prompt: &str,
) -> Map<String, Value> {
    let mut payload = base("SubagentStart", session_id, cwd);
    payload.insert("agent_name".into(), Value::String(agent_name.into()));
    payload.insert("prompt".into(), Value::String(prompt.into()));
    payload
}

/// SubagentStop hook payload.
pub fn subagent_stop(
    session_id: &str,
    cwd: &str,
    agent_name: &str,
    response: &str,
) -> Map<String, Value> {
    let mut payload = base("SubagentStop", session_id, cwd);
    payload.insert("agent_name".into(), Value::String(agent_name.into()));
    payload.insert("response".into(), Value::String(response.into()));
    payload
}

/// PreCompact hook payload.
pub fn pre_compact(
    session_id: &str,
    cwd: &str,
    trigger: &str,
    token_count: usize,
) -> Map<String, Value> {
    let mut payload = base("PreCompact", session_id, cwd);
    payload.insert("trigger".into(), Value::String(trigger.into()));
    payload.insert("token_count".into(), Value::Number(token_count.into()));
    payload
}

/// PostCompact hook payload.
pub fn post_compact(
    session_id: &str,
    cwd: &str,
    trigger: &str,
    estimated_token_count: usize,
) -> Map<String, Value> {
    let mut payload = base("PostCompact", session_id, cwd);
    payload.insert("trigger".into(), Value::String(trigger.into()));
    payload.insert(
        "estimated_token_count".into(),
        Value::Number(estimated_token_count.into()),
    );
    payload
}

/// Notification hook payload.
pub fn notification(
    session_id: &str,
    cwd: &str,
    sink: &str,
    notification_type: &str,
    title: &str,
    body: &str,
    severity: &str,
) -> Map<String, Value> {
    let mut payload = base("Notification", session_id, cwd);
    payload.insert("sink".into(), Value::String(sink.into()));
    payload.insert(
        "notification_type".into(),
        Value::String(notification_type.into()),
    );
    payload.insert("title".into(), Value::String(title.into()));
    payload.insert("body".into(), Value::String(body.into()));
    payload.insert("severity".into(), Value::String(severity.into()));
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_tool_use_payload() {
        let mut input = Map::new();
        input.insert("command".into(), Value::String("echo hi".into()));
        let payload = pre_tool_use("s1", "/tmp", "Shell", &input, "tc1");
        assert_eq!(payload["hook_event_name"], "PreToolUse");
        assert_eq!(payload["session_id"], "s1");
        assert_eq!(payload["tool_name"], "Shell");
    }

    #[test]
    fn stop_payload_default() {
        let payload = stop("s1", "/tmp", false);
        assert_eq!(payload["hook_event_name"], "Stop");
        assert_eq!(payload["stop_hook_active"], false);
    }

    #[test]
    fn notification_payload() {
        let payload = notification("s1", "/tmp", "desktop", "info", "title", "body", "warning");
        assert_eq!(payload["sink"], "desktop");
        assert_eq!(payload["severity"], "warning");
    }
}
