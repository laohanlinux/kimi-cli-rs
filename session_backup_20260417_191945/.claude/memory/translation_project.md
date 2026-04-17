---
name: Kimi CLI Rust Translation Project
description: Converting the kimi-cli Python codebase into idiomatic Rust with tracing and English comments
type: project
---

We are translating the entire `kimi-cli` Python project (`/Users/rg/Projects/kimi-cli/src/kimi_cli`) into a Rust crate at `/Users/rg/Projects/kimi-cli-rs/`.

**Why:** The user requested a complete line-by-line translation with architecture analysis, detailed diagrams, English comments, Rust idioms, and `tracing` integration for timing main function calls.

**How to apply:**
- Every module from `kimi-cli/src/kimi_cli` must have a corresponding Rust file under `kimi-cli-rs/src/`.
- All significant structs/functions need English doc comments (`///`).
- All major public methods must be annotated with `#[tracing::instrument]`.
- Use `tokio`, `clap`, `axum`, `serde`, `thiserror`, `tracing`, `ratatui`, `reqwest`.
- Replace Python exceptions with `Result<T, E>`.
- Replace Python `ContextVar` with explicit `Arc` passing or `tokio::task_local!`.

**Progress tracking:**
- Analysis diagrams saved to `/Users/rg/Projects/kimi-cli-rs/analysis/`.
- Translation plan saved to `/Users/rg/Projects/kimi-cli-rs/translation_plan/`.
- **Current Phase:** 11 — Core systems are feature-complete stubs; remaining large modules are wire server, web API, vis pipeline, rich UI shell, notifications, background store, ACP handlers, plugin manager, and CLI commands.
- **Compilation status:** `cargo build` succeeds for both library and binary. `cargo test` passes 66 unit tests + 5 integration tests.

**Recently completed (2026-04-16):**
- **Approval System & Message Helpers:**
  - Full `ApprovalResult`, `ApprovalState`, and `ApprovalRuntime` with create/wait/resolve/cancel lifecycle
  - `system()`, `system_reminder()`, `is_system_reminder_message()`, `tool_result_to_message()`, `check_message()`
- **Context & Toolset Improvements:**
  - `Context::clear()`, batch `append_messages()`, `checkpoint(add_user_message)`, atomic system-prompt prepend, robust JSONL parsing
  - `CURRENT_TOOL_CALL` task-local context, `unhide()`, rich `PreToolUse`/`PostToolUse` hook data
- **KimiSoul Core & Slash Commands:**
  - Auto-compaction, hook events (`UserPromptSubmit`, `Stop`, `PreCompact`, `PostCompact`), title generation, steer injection
  - Wire `StatusUpdate`/text streaming for `/compact`, `/clear`, `/plan`, `/yolo`
- **Background Tasks:**
  - Fleshed out `BackgroundTaskManager` with `get_task`, `kill`, `wait`, `read_output`, `resolve_output_path`
- **Tools Translation:**
  - `AskUserQuestion` via wire `QuestionRequest`/`QuestionResponse`
  - `EnterPlanMode` and `ExitPlanMode` with YOLO auto-approve and wire question flows
  - `TaskList`, `TaskOutput`, `TaskStop` with root checks, approval, block/timeout, and rich output formatting
- **Wire Protocol:**
  - Added `ApprovalResponse`, `QuestionResponse`, `HookRequest`, `DisplayBlock` support
- **Config / Session / Metadata:**
  - JSON→TOML migration, `Theme` enum, JSON save support
  - Session create assertion, `.jsonl` legacy detection, detailed exception handling
  - Metadata kaos-aware `sessions_dir`, atomic JSON writes via `utils::io::atomic_json_write`

**Previously completed:**
- Foundation modules: `error`, `constant`, `share`, `config`, `metadata`, `session_state`, `session`, `wire/*`
- Soul Core: `soul/mod`, `agent`, `context`, `message`, `toolset`, `kimisoul`, `slash`, `compaction`, `dynamic_injection`, `approval`, `btw`, `denwa_renji`
- LLM + Skill modules + flow diagram stubs
- Tools: `tools/mod`, `file/*`, `shell`, `web`, `ask_user`, `plan`, `think`, `todo`, `background`, `dmail`
- UI: `ui/mod`, `acp`, `print`, `shell`, `theme`
- Servers: `web/mod` (with Axum router), `vis/mod`, `acp/mod`
- Entrypoint: `app/mod.rs` (`KimiCLI`), `cli/mod.rs` (clap CLI), `main.rs` (tokio main)
- Runtime Extensions: `notifications/manager.rs`, `approval_runtime/runtime.rs`, `hooks/engine.rs`, `auth/oauth.rs`
- Plugin system: `plugin/mod.rs`
- Background tasks: `background/manager.rs`

**Next up:**
- `wire/server.rs` full JSON-RPC/WebSocket wire server
- `web/*` (app.rs, auth.rs, models.rs, runner/process.rs, runner/worker.rs, store/sessions.rs)
- `vis/*` visualization pipeline
- `ui/shell/*` and `ui/print/*` rich interactive features
- `notifications/*` full queue and delivery logic
- `utils/*` remaining utility modules (diff, clipboard, editor, export, file_filter, frontmatter, logging, sensitive, server, signals, string, subprocess_env, term, datetime, broadcast, changelog)
- `background/store.rs` persistence layer
- `acp/*` full RPC handlers
- `plugin/manager.rs` + `plugin/tool.rs`
- `cli/*` full command implementations
