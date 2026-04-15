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
- **Current Phase:** 5 complete — all major module groups have compilable Rust stubs. Core tool implementations filled in. Runtime extension stubs implemented.
- **Compilation status:** `cargo build` succeeds for both library and binary. `cargo test` passes 58 unit tests + 5 integration tests.

**Recently completed (2026-04-15):**
- Foundation modules: `error`, `constant`, `share`, `config`, `metadata`, `session_state`, `session`, `wire/*`
  - Added `load_config_from_string` in `src/config.rs` for TOML/JSON inline config parsing
  - Added `WireFile::records()` in `src/wire/file.rs` to read wire message records
  - Completed `Session::refresh()` to derive titles from the first `TurnBegin` record
- Soul Core: `soul/mod`, `agent`, `context`, `message`, `toolset`, `kimisoul`, `slash`, `compaction`, `dynamic_injection`, `approval`, `btw`, `denwa_renji`
  - Fixed `KimiSoul::set_hook_engine` to bind the hook engine into `KimiToolset`
  - Added `KimiToolset::start_background_mcp_loading` and `wait_for_background_mcp_loading` stubs
  - Wrapped `Runtime.labor_market` in `Arc<RwLock<...>>` so `load_agent` can register builtin subagent types
- LLM + Skill modules + flow diagram stubs
- Tools: `tools/mod`, `file/*`, `shell`, `web`, `ask_user`, `plan`, `think`, `todo`, `background`, `dmail`
- UI: `ui/mod`, `acp`, `print`, `shell`, `theme`
- Servers: `web/mod` (with Axum router), `vis/mod`, `acp/mod`
- Entrypoint: `app/mod.rs` (`KimiCLI`), `cli/mod.rs` (clap CLI), `main.rs` (tokio main)
- Utils: `utils/mod.rs`
- Runtime Extensions: `notifications/manager.rs` (mpsc queue), `approval_runtime/runtime.rs` (wildcard rules), `hooks/engine.rs` (HookDef execution), `auth/oauth.rs` (file-based tokens)
- Plugin system: `plugin/mod.rs` with TOML manifest parsing, `PluginTool` shell-command wrapper with `{arg}` placeholder substitution, and `load_plugin_tools` directory scanning
- Background tasks: `background/manager.rs` with `tokio::process::Child` tracking, `spawn` with `max_running_tasks` limit enforcement, and `stop` that kills the child process
- All missing submodule stubs created so `cargo check` passes with only warnings
- **55 unit tests** covering `config`, `share`, `metadata`, `session_state`, `tools::extract_key_argument`, `file` helpers, `shell`, `web`, `auth::oauth`, `approval_runtime`, `hooks`, `plugin`, `background`
- **Implemented working tool logic** for:
  - `ReadFile` (with line offsets, tail mode, max lines/bytes, truncation)
  - `WriteFile` (overwrite / append modes)
  - `StrReplaceFile` (single and multiple edits)
  - `Glob` (pattern matching with directory filter)
  - `Grep` (regex-based file content search with multiple output modes)
  - `ReadMediaFile` (base64 encoding)
  - `Shell` (foreground execution via `tokio::process::Command` with timeout)
  - `FetchUrl` (HTTP GET via `reqwest`)

**Next up:**
- MCP tool loading integration
- Additional polish and edge-case handling
