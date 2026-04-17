use serde_json::Value;

pub mod agent;
pub mod ask_user;
pub mod background;
pub mod display;
pub mod dmail;
pub mod file;
pub mod plan;
pub mod shell;
pub mod think;
pub mod todo;
pub mod utils;
pub mod web;

/// Raised when a tool decides to skip itself from the loading process.
#[derive(Debug, Clone)]
pub struct SkipThisTool;

impl std::fmt::Display for SkipThisTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool skipped during loading")
    }
}

impl std::error::Error for SkipThisTool {}

/// Extracts a short key argument from JSON tool arguments for display.
pub fn extract_key_argument(json_content: &str, tool_name: &str) -> Option<String> {
    let args: Value = serde_json::from_str(json_content).ok()?;
    let args = args.as_object()?;
    if args.is_empty() {
        return None;
    }

    let key = match tool_name {
        "Agent" => args.get("description")?.as_str()?.to_string(),
        "SendDMail" => return None,
        "Think" => args.get("thought")?.as_str()?.to_string(),
        "SetTodoList" => return None,
        "Shell" => args.get("command")?.as_str()?.to_string(),
        "TaskOutput" => args.get("task_id")?.as_str()?.to_string(),
        "TaskList" => {
            if args
                .get("active_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
            {
                "active".into()
            } else {
                "all".into()
            }
        }
        "TaskStop" => args.get("task_id")?.as_str()?.to_string(),
        "ReadFile" => normalize_path(args.get("path")?.as_str()?),
        "ReadMediaFile" => normalize_path(args.get("path")?.as_str()?),
        "Glob" => args.get("pattern")?.as_str()?.to_string(),
        "Grep" => args.get("pattern")?.as_str()?.to_string(),
        "WriteFile" => normalize_path(args.get("path")?.as_str()?),
        "StrReplaceFile" => normalize_path(args.get("path")?.as_str()?),
        "SearchWeb" => args.get("query")?.as_str()?.to_string(),
        "FetchURL" => args.get("url")?.as_str()?.to_string(),
        _ => json_content.to_string(),
    };

    Some(crate::utils::string::shorten_middle(&key, 50))
}

fn normalize_path(path: &str) -> String {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    if let Some(stripped) = path.strip_prefix(&cwd) {
        stripped.trim_start_matches(['/', '\\']).to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_key_argument_shell() {
        let json = r#"{"command":"echo hello"}"#;
        assert_eq!(
            extract_key_argument(json, "Shell"),
            Some("echo hello".into())
        );
    }

    #[test]
    fn extract_key_argument_read_file() {
        let json = r#"{"path":"/tmp/foo.txt"}"#;
        let out = extract_key_argument(json, "ReadFile").unwrap();
        assert!(out.ends_with("foo.txt"));
    }

    #[test]
    fn extract_key_argument_think() {
        let json = r#"{"thought":"I should plan"}"#;
        assert_eq!(
            extract_key_argument(json, "Think"),
            Some("I should plan".into())
        );
    }

    #[test]
    fn extract_key_argument_unknown_uses_raw() {
        let json = r#"{"x":1}"#;
        assert_eq!(
            extract_key_argument(json, "UnknownTool"),
            Some(r#"{"x":1}"#.into())
        );
    }

    #[test]
    fn extract_key_argument_empty_args() {
        let json = r#"{}"#;
        assert_eq!(extract_key_argument(json, "Shell"), None);
    }

    #[test]
    fn normalize_path_strips_cwd() {
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let path = format!("{}/src/main.rs", cwd);
        assert_eq!(normalize_path(&path), "src/main.rs");
    }

    #[test]
    fn normalize_path_leaves_absolute() {
        let path = "/tmp/foo.rs";
        assert_eq!(normalize_path(path), "/tmp/foo.rs");
    }
}
