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
- **Current Phase:** 4 complete — all major module groups have compilable Rust stubs. Core tool implementations filled in. 32 unit tests passing.
- **Compilation status:** `cargo build` succeeds for both library and binary. `cargo test` passes 32 unit tests.

**Recently completed (2026-04-15):**
- Foundation modules: `error`, `constant`, `share`, `config`, `metadata`, `session_state`, `session`, `wire/*`
- Soul Core: `soul/mod`, `agent`, `context`, `message`, `toolset`, `kimisoul`, `slash`, `compaction`, `dynamic_injection`, `approval`
- LLM + Skill modules + flow diagram stubs
- Tools: `tools/mod`, `file/*`, `shell`, `web`, `ask_user`, `plan`, `think`, `todo`, `background`, `dmail`
- UI: `ui/mod`, `acp`, `print`, `shell`
- Servers: `web/mod` (with Axum router), `vis/mod`, `acp/mod`
- Entrypoint: `app/mod.rs` (`KimiCLI`), `cli/mod.rs` (clap CLI), `main.rs` (tokio main)
- Utils: `utils/mod.rs`
- All missing submodule stubs created so `cargo check` passes with only warnings
- **Added 32 unit tests** covering `config`, `share`, `metadata`, `session_state`, `tools::extract_key_argument`, `file` helpers, `shell`, and `web`
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
- TUI shell implementation with `ratatui`
- Web server full handler implementation
- Additional integration tests as UI and server logic mature
