# Phase 2: Soul Core Translation Plan

## 2.1 `src/soul/message.rs`

**Strategy:** Replace `kosong.message.Message` with a native Rust enum/struct hierarchy.

```rust
use serde::{Deserialize, Serialize};

/// A content part within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Think { thought: String },
    ImageUrl { url: String },
    AudioUrl { url: String },
    VideoUrl { url: String },
}

/// A chat message for the LLM context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user", "assistant", "tool", "system"
    pub content: Vec<ContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Extracts all text parts joined by the given separator.
    #[tracing::instrument(level = "trace")]
    pub fn extract_text(&self, sep: &str) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(sep)
    }
}

/// An LLM-requested tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub return_value: ToolReturnValue,
}

/// Discriminated union for tool return values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolReturnValue {
    Ok { output: String, message: Option<String> },
    Error { error: String },
}
```

## 2.2 `src/soul/context.rs`

**Strategy:** Maintain a `Vec<Message>` in memory and append JSONL lines to disk.

```rust
use std::io::Write;
use std::path::PathBuf;

/// Persistent conversation context backed by a JSONL file.
pub struct Context {
    history: Vec<Message>,
    token_count: usize,
    pending_token_estimate: usize,
    next_checkpoint_id: usize,
    system_prompt: Option<String>,
    file_backend: PathBuf,
}

impl Context {
    /// Restores the context from the JSONL file.
    #[tracing::instrument(level = "debug")]
    pub async fn restore(&mut self) -> crate::Result<()> {
        if !self.file_backend.exists() {
            return Ok(());
        }
        let text = tokio::fs::read_to_string(&self.file_backend).await?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.apply_record(line)?;
        }
        Ok(())
    }

    fn apply_record(&mut self, line: &str) -> crate::Result<()> {
        let mut record: serde_json::Value = serde_json::from_str(line)?;
        let role = record.get("role").and_then(|v| v.as_str());
        match role {
            Some("_system_prompt") => {
                self.system_prompt = record.get("content").and_then(|v| v.as_str()).map(String::from);
            }
            Some("_usage") => {
                self.token_count = record.get("token_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.pending_token_estimate = 0;
            }
            Some("_checkpoint") => {
                self.next_checkpoint_id = record.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize + 1;
            }
            _ => {
                let msg: Message = serde::from_value(record)?;
                self.history.push(msg);
            }
        }
        Ok(())
    }

    /// Appends a message to memory and the JSONL file.
    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn append_message(&mut self, message: &Message) -> crate::Result<()> {
        self.pending_token_estimate += estimate_text_tokens(&message.extract_text(" "));
        self.history.push(message.clone());
        let line = serde_json::to_string(message)?;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Persists the current token count as a usage marker.
    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn update_token_count(&mut self, count: usize) -> crate::Result<()> {
        self.token_count = count;
        self.pending_token_estimate = 0;
        let record = serde_json::json!({"role": "_usage", "token_count": count});
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(serde_json::to_string(&record)?.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// Creates a checkpoint marker in the file.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn checkpoint(&mut self) -> crate::Result<()> {
        let id = self.next_checkpoint_id;
        self.next_checkpoint_id += 1;
        let record = serde_json::json!({"role": "_checkpoint", "id": id});
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&self.file_backend)
            .await?;
        file.write_all(serde_json::to_string(&record)?.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}

/// Naive token estimator for English text (~4 chars/token).
fn estimate_text_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}
```

## 2.3 `src/soul/agent.rs`

**Strategy:** `Runtime` becomes an `Arc<RuntimeInner>` or a struct with `Arc` fields.

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

/// Runtime dependency container shared across the system.
#[derive(Debug, Clone)]
pub struct Runtime {
    pub config: crate::config::Config,
    pub llm: Option<crate::llm::Llm>,
    pub session: crate::session::Session,
    pub builtin_args: BuiltinSystemPromptArgs,
    pub approval: crate::soul::approval::Approval,
    pub labor_market: crate::subagents::registry::LaborMarket,
    pub notifications: crate::notifications::manager::NotificationManager,
    pub background_tasks: crate::background::manager::BackgroundTaskManager,
    pub skills: std::collections::HashMap<String, crate::skill::Skill>,
    pub subagent_store: Arc<RwLock<crate::subagents::store::SubagentStore>>,
    pub approval_runtime: Option<Arc<crate::approval_runtime::runtime::ApprovalRuntime>>,
    pub root_wire_hub: Option<Arc<crate::wire::root_hub::RootWireHub>>,
    pub hook_engine: Option<crate::hooks::engine::HookEngine>,
    pub oauth: crate::auth::oauth::OAuthManager,
}

impl Runtime {
    /// Factory that bootstraps the runtime.
    #[tracing::instrument(level = "debug")]
    pub async fn create(
        config: crate::config::Config,
        oauth: crate::auth::oauth::OAuthManager,
        llm: Option<crate::llm::Llm>,
        session: crate::session::Session,
        yolo: bool,
        skills_dirs: Option<Vec<std::path::PathBuf>>,
    ) -> crate::Result<Self> {
        // ... initialize managers ...
        Ok(Runtime { /* ... */ })
    }
}

/// An instantiated agent specification.
#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub system_prompt: String,
    pub toolset: crate::soul::toolset::KimiToolset,
    pub runtime: Runtime,
}

/// Loads an agent from the YAML spec file.
#[tracing::instrument(level = "debug")]
pub async fn load_agent(
    agent_file: &std::path::Path,
    runtime: &Runtime,
    mcp_configs: Vec<crate::config::McpConfig>,
    start_mcp_loading: bool,
) -> crate::Result<Agent> {
    // ... parse YAML, build toolset, load tools, load MCP tools ...
    todo!("implement agent loading")
}
```

## 2.4 `src/soul/toolset.rs`

**Strategy:** Tool registry with `Box<dyn Tool>` trait objects. Replace Python dynamic `__init__` injection with a dependency map or builder pattern.

```rust
use std::collections::HashMap;

/// Trait implemented by every tool.
#[async_trait::async_trait]
pub trait Tool: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    /// Executes the tool with the given arguments.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn call(&self, arguments: serde_json::Value) -> crate::soul::message::ToolReturnValue;
}

/// Central registry and executor for all tools.
#[derive(Default)]
pub struct KimiToolset {
    tools: HashMap<String, Box<dyn Tool>>,
    hidden: std::collections::HashSet<String>,
    hook_engine: Option<crate::hooks::engine::HookEngine>,
}

impl KimiToolset {
    pub fn add(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn hide(&mut self, name: &str) {
        self.hidden.insert(name.to_string());
    }

    /// Looks up a tool and executes it.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn handle(
        &self,
        tool_call: &crate::soul::message::ToolCall,
    ) -> crate::soul::message::ToolResult {
        let Some(tool) = self.tools.get(&tool_call.name) else {
            return crate::soul::message::ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: crate::soul::message::ToolReturnValue::Error {
                    error: format!("Tool '{}' not found", tool_call.name),
                },
            };
        };

        // Parse arguments.
        let args = tool_call.arguments.clone();

        // PreToolUse hook.
        if let Some(ref engine) = self.hook_engine {
            match engine.trigger("PreToolUse", &tool_call.name, args.clone()).await {
                Ok(crate::hooks::engine::HookAction::Block { reason }) => {
                    return crate::soul::message::ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        return_value: crate::soul::message::ToolReturnValue::Error {
                            error: format!("Blocked by hook: {reason}"),
                        },
                    };
                }
                _ => {}
            }
        }

        // Execute with timing.
        let start = std::time::Instant::now();
        let result = tool.call(args).await;
        let elapsed = start.elapsed();
        tracing::info!(tool = %tool_call.name, ?elapsed, "tool executed");

        // PostToolUse hook (fire-and-forget).
        if let Some(ref engine) = self.hook_engine {
            let engine = engine.clone();
            let name = tool_call.name.clone();
            tokio::spawn(async move {
                let _ = engine.trigger("PostToolUse", &name, serde_json::json!({})).await;
            });
        }

        crate::soul::message::ToolResult {
            tool_call_id: tool_call.id.clone(),
            return_value: result,
        }
    }
}
```

## 2.5 `src/soul/kimi_soul.rs`

**Strategy:** Translate the main loop to an `async` struct with `tokio::sync` primitives.

```rust
use tokio::sync::{mpsc, Mutex};

/// Reasons a turn may stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStopReason {
    NoToolCalls,
    ToolRejected,
    MaxStepsReached,
    Cancelled,
}

/// Outcome of a single turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub stop_reason: TurnStopReason,
    pub final_message: Option<crate::soul::message::Message>,
    pub step_count: usize,
}

/// The core agent execution engine.
pub struct KimiSoul {
    agent: crate::soul::agent::Agent,
    context: crate::soul::context::Context,
    plan_mode: bool,
    plan_session_id: Option<String>,
    steer_queue: mpsc::UnboundedReceiver<String>,
    // ... other fields
}

impl KimiSoul {
    /// Runs a single user turn.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        user_input: Vec<crate::soul::message::ContentPart>,
    ) -> crate::Result<TurnOutcome> {
        // ... turn logic matching Python KimiSoul.run() ...
        todo!("implement run")
    }

    /// Internal step loop.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn agent_loop(&mut self) -> crate::Result<TurnStopReason> {
        // ... step iteration, compaction, llm call, tool execution ...
        todo!("implement agent_loop")
    }

    /// Executes one LLM step.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn step(&mut self) -> crate::Result<Option<TurnStopReason>> {
        // ... deliver notifications, inject dynamics, call LLM, grow context ...
        todo!("implement step")
    }
}
```

## 2.6 `src/soul/mod.rs` (from `soul/__init__.py`)

```rust
use tokio::sync::{broadcast, mpsc};

/// Orchestrates a soul run with its UI loop and notification pump.
#[tracing::instrument(level = "info", skip_all)]
pub async fn run_soul(
    soul: &mut KimiSoul,
    user_input: Vec<crate::soul::message::ContentPart>,
    ui_loop_fn: impl FnOnce(crate::wire::Wire) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    cancel_event: tokio::sync::watch::Receiver<bool>,
    runtime: &crate::soul::agent::Runtime,
) -> crate::Result<TurnOutcome> {
    let wire = crate::wire::Wire::new();
    // ... spawn ui_loop, soul_task, notification pump ...
    // ... await cancellation or completion ...
    todo!("implement run_soul orchestrator")
}
```

## Tracing Strategy for Soul Core
- `KimiSoul::run` → `#[tracing::instrument(level = "info")]` — logs every user turn.
- `KimiSoul::agent_loop` → `#[tracing::instrument(level = "debug")]` — logs step counts and stop reasons.
- `KimiSoul::step` → `#[tracing::instrument(level = "debug")]` — logs LLM latency, token usage.
- `KimiToolset::handle` → `#[tracing::instrument(level = "debug")]` — logs tool name and execution time.
- `Context::append_message`, `Context::checkpoint`, `Context::restore` → `#[tracing::instrument(level = "trace")]`.
