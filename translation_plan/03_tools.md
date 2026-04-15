# Phase 3: Tools Translation Plan

## General Tool Architecture in Rust

All tools live under `src/tools/` mirroring the Python `tools/` directory.

### Tool Trait (reused from `soul/toolset.rs`)
```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue;
}
```

## 3.1 File Tools (`src/tools/file/`)

### `read.rs` — ReadFile
```rust
use std::path::PathBuf;

/// Reads a text file with offset and line limits.
pub struct ReadFile;

#[async_trait::async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Read the contents of a file." }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "line_offset": { "type": "integer", "default": 1 },
                "limit": { "type": "integer", "default": 1000 }
            },
            "required": ["file_path"]
        })
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let path = arguments.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        let path = PathBuf::from(path);
        // Validate workspace bounds, detect binary, apply limits, format with line numbers.
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                // ... apply line_offset / limit logic ...
                crate::soul::message::ToolReturnValue::Ok {
                    output: content,
                    message: None,
                }
            }
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Failed to read file: {e}"),
            },
        }
    }
}
```

### `write.rs` — WriteFile
```rust
/// Writes or appends content to a file.
pub struct WriteFile {
    runtime: crate::soul::agent::Runtime,
}

impl WriteFile {
    pub fn new(runtime: crate::soul::agent::Runtime) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str { "write_file" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let path = arguments["file_path"].as_str().unwrap_or("");
        let content = arguments["content"].as_str().unwrap_or("");
        let overwrite = arguments["overwrite"].as_bool().unwrap_or(false);
        // Approval flow (except plan mode auto-approve)
        // Write file, return diff summary.
        todo!("implement write_file")
    }
}
```

### `replace.rs` — StrReplaceFile
```rust
/// Performs string replacement edits in a file.
pub struct StrReplaceFile;

#[async_trait::async_trait]
impl Tool for StrReplaceFile {
    fn name(&self) -> &str { "str_replace_file" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        // Parse edits, validate, apply sequentially, compute diff.
        todo!("implement str_replace_file")
    }
}
```

### `glob.rs` — Glob
```rust
/// Finds files matching a glob pattern within the workspace.
pub struct Glob;

#[async_trait::async_trait]
impl Tool for Glob {
    fn name(&self) -> &str { "glob" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let pattern = arguments["pattern"].as_str().unwrap_or("");
        let dir = arguments["directory"].as_str().unwrap_or(".");
        // Use `glob::glob` or `wax` crate for pattern matching.
        let matches: Vec<String> = glob::glob(&format!("{}/{}", dir, pattern))
            .unwrap()
            .filter_map(Result::ok)
            .map(|p| p.display().to_string())
            .collect();
        crate::soul::message::ToolReturnValue::Ok {
            output: matches.join("\n"),
            message: None,
        }
    }
}
```

### `grep_local.rs` — Grep
```rust
/// Executes ripgrep against the workspace.
pub struct Grep;

#[async_trait::async_trait]
impl Tool for Grep {
    fn name(&self) -> &str { "grep" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let pattern = arguments["pattern"].as_str().unwrap_or("");
        let output = tokio::process::Command::new("rg")
            .arg(pattern)
            .output()
            .await;
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                crate::soul::message::ToolReturnValue::Ok {
                    output: text,
                    message: None,
                }
            }
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("rg failed: {e}"),
            },
        }
    }
}
```

## 3.2 Shell Tool (`src/tools/shell.rs`)

```rust
/// Executes a bash / shell command, optionally in the background.
pub struct Shell {
    runtime: crate::soul::agent::Runtime,
}

#[async_trait::async_trait]
impl Tool for Shell {
    fn name(&self) -> &str { "shell" }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let command = arguments["command"].as_str().unwrap_or("");
        let background = arguments["run_in_background"].as_bool().unwrap_or(false);
        let timeout = arguments["timeout"].as_u64().unwrap_or(300);

        if background {
            let task = self.runtime.background_tasks.create_bash_task(command, timeout).await;
            return crate::soul::message::ToolReturnValue::Ok {
                output: format!("Background task started: {}", task.id),
                message: None,
            };
        }

        // Foreground execution with approval.
        let start = std::time::Instant::now();
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .kill_on_drop(true)
            .timeout(std::time::Duration::from_secs(timeout))
            .output()
            .await;
        let elapsed = start.elapsed();
        tracing::info!(?elapsed, "shell command completed");

        match output {
            Ok(out) if out.status.success() => crate::soul::message::ToolReturnValue::Ok {
                output: String::from_utf8_lossy(&out.stdout).to_string(),
                message: None,
            },
            Ok(out) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Exit code {}: {}", out.status, String::from_utf8_lossy(&out.stderr)),
            },
            Err(e) => crate::soul::message::ToolReturnValue::Error {
                error: format!("Command failed: {e}"),
            },
        }
    }
}
```

## 3.3 Web Tools (`src/tools/web/`)

### `search.rs`
```rust
/// Searches the web via Moonshot Search service.
pub struct SearchWeb {
    config: crate::config::MoonshotSearchConfig,
}

#[async_trait::async_trait]
impl Tool for SearchWeb {
    fn name(&self) -> &str { "search_web" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let query = arguments["text_query"].as_str().unwrap_or("");
        let client = reqwest::Client::new();
        let res = client
            .post(&self.config.base_url)
            .bearer_auth(self.config.api_key.expose_secret())
            .json(&serde_json::json!({"text_query": query, "limit": 10}))
            .send()
            .await;
        match res {
            Ok(r) => match r.text().await {
                Ok(body) => crate::soul::message::ToolReturnValue::Ok { output: body, message: None },
                Err(e) => crate::soul::message::ToolReturnValue::Error { error: e.to_string() },
            },
            Err(e) => crate::soul::message::ToolReturnValue::Error { error: e.to_string() },
        }
    }
}
```

### `fetch.rs`
```rust
/// Fetches a URL and returns its content as markdown or plain text.
pub struct FetchUrl;

#[async_trait::async_trait]
impl Tool for FetchUrl {
    fn name(&self) -> &str { "fetch_url" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        let url = arguments["url"].as_str().unwrap_or("");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .unwrap();
        match client.get(url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(text) => crate::soul::message::ToolReturnValue::Ok { output: text, message: None },
                Err(e) => crate::soul::message::ToolReturnValue::Error { error: e.to_string() },
            },
            Err(e) => crate::soul::message::ToolReturnValue::Error { error: e.to_string() },
        }
    }
}
```

## 3.4 Special Tools

### `ask_user.rs`
```rust
/// Sends an interactive question to the user via the Wire protocol.
pub struct AskUserQuestion;

#[async_trait::async_trait]
impl Tool for AskUserQuestion {
    fn name(&self) -> &str { "ask_user_question" }
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        // Build QuestionRequest, send over wire, await response.
        todo!("implement ask_user_question wire integration")
    }
}
```

### `plan/enter.rs` & `plan/exit.rs`
```rust
/// Enters plan mode after user confirmation.
pub struct EnterPlanMode;
/// Exits plan mode after reviewing the plan.
pub struct ExitPlanMode;
```

### `background.rs`
```rust
/// Lists, reads output from, or stops background tasks.
pub struct TaskList;
pub struct TaskOutput;
pub struct TaskStop;
```

### `think.rs`
```rust
/// No-op reasoning tool.
pub struct Think;

#[async_trait::async_trait]
impl Tool for Think {
    fn name(&self) -> &str { "think" }
    async fn call(&self, _arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue {
        crate::soul::message::ToolReturnValue::Ok {
            output: String::new(),
            message: Some("Thought logged".into()),
        }
    }
}
```

### `todo.rs`
```rust
/// Reads or writes the agent's todo list.
pub struct SetTodoList;
```

### `dmail.rs`
```rust
/// Sends a D-Mail to revert context to a previous checkpoint.
pub struct SendDMail;
```

## Tracing Strategy for Tools
- Every `Tool::call` implementation gets `#[tracing::instrument(level = "debug")]`.
- Record `elapsed_ms` for I/O-bound tools (file, shell, web).
- Record argument counts (e.g., number of edits in `StrReplaceFile`).
- Use `tracing::warn!` for approval rejections.
