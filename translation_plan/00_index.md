# Kimi CLI Rust Translation Plan

## Objective
Translate the entire `kimi-cli` Python codebase (`/Users/rg/Projects/kimi-cli/src/kimi_cli`) into idiomatic Rust, line-by-line, with English comments and `tracing` integration for timing major function calls.

## Target Crate
`kimi-cli-rs` at `/Users/rg/Projects/kimi-cli-rs/`

## Rust Stack
- **Async runtime**: `tokio` (full features)
- **Serialization**: `serde` + `toml` + `serde_json`
- **Configuration**: `config` crate or custom TOML parser with `serde`
- **CLI**: `clap` (derive-based, subcommands, lazy loading via `Box::new`)
- **HTTP client**: `reqwest` (async, rustls-tls)
- **Web framework**: `axum` (instead of FastAPI)
- **WebSockets**: `tokio-tungstenite` or `axum::extract::ws`
- **TUI / prompt**: `ratatui` + `crossterm` (replacing prompt_toolkit)
- **Rich terminal output**: `ansi_term` / `owo-colors` / custom `ratatui` widgets
- **Markdown rendering**: `pulldown-cmark` + custom block renderer
- **Diff rendering**: `similar` or `diffy`
- **Path / workspace**: `std::path::PathBuf` + `dunce`
- **Logging / tracing**: `tracing` + `tracing-subscriber` + `opentelemetry` optional
- **Timing**: `tracing::instrument` with `level = "debug"` on every major function
- **Process spawning**: `tokio::process::Command`
- **File watching**: `notify` crate
- **Regex**: `regex`
- **Base64 / media**: `base64` + `image` crate
- **Archive**: `zip` crate
- **OAuth / JWT**: `oauth2` crate
- **State machine**: `strum` for enums
- **Error handling**: `thiserror` + `color-eyre` at application boundaries
- **Context variables**: `tokio::task_local!` or explicit `Arc<Runtime>` passing

## Translation Principles
1. **Every line translated**: No skipping modules. Tools, UI, servers, auth, hooks, etc. all included.
2. **English comments**: Every struct, function, and significant block must have English doc comments (`///`).
3. **Idiomatic Rust**:
   - Use `Result<T, E>` instead of exceptions.
   - Use `async`/`await` with `tokio`.
   - Use `Arc`/`RwLock` for shared mutable state across tasks.
   - Use `enum` + `match` instead of Python `match` / unions.
   - Use `trait` objects (`dyn Trait`) or generics where Python uses protocols.
   - Replace Python `ContextVar` with explicit `Arc` passing or `tokio::task_local!`.
4. **Tracing integration**:
   - `#[tracing::instrument]` on all public methods and significant internals.
   - Use `tracing::info_span!` + `span.enter()` for explicit timing blocks.
   - Custom `tracing::Layer` to emit slow-call warnings (> 1s).
5. **Testing**: Add `#[cfg(test)]` modules for pure logic; integration tests for tool I/O.

## High-Level Module Mapping

| Python Module | Rust Target Module(s) | Notes |
|---------------|----------------------|-------|
| `__main__.py` | `src/main.rs` | CLI entrypoint |
| `app.py` | `src/app.rs` | `KimiCLI` struct + factory |
| `session.py` | `src/session.rs` | `Session` struct + persistence |
| `config.py` | `src/config.rs` | Pydantic -> `serde` structs |
| `llm.py` | `src/llm.rs` | LLM wrapper; replace `kosong` with `async-openai` or custom provider |
| `agentspec.py` | `src/agent_spec.rs` | YAML agent spec parsing |
| `soul/kimisoul.py` | `src/soul/kimi_soul.rs` | Core agent loop |
| `soul/agent.py` | `src/soul/agent.rs` | `Agent`, `Runtime` |
| `soul/context.py` | `src/soul/context.rs` | Context history manager |
| `soul/toolset.py` | `src/soul/toolset.rs` | Tool registry + executor |
| `soul/message.py` | `src/soul/message.rs` | Message types |
| `soul/slash.py` | `src/soul/slash.rs` | Slash commands |
| `soul/compaction.py` | `src/soul/compaction.rs` | Context compaction |
| `soul/dynamic_injection.rs` | `src/soul/dynamic_injection.rs` | Injection providers |
| `soul/dynamic_injections/*.py` | `src/soul/dynamic_injections/*.rs` | Plan mode, YOLO mode |
| `soul/btw.py` | `src/soul/btw.rs` | BTW notifications |
| `soul/denwarenji.py` | `src/soul/denwa_renji.rs` | Time travel / D-Mail |
| `soul/approval.py` | `src/soul/approval.rs` | Approval facade |
| `soul/kimisoul.py` | `src/soul/mod.rs` | `run_soul` orchestrator |
| `wire/*.py` | `src/wire/*.rs` | Wire protocol |
| `tools/**/*.py` | `src/tools/**/*.rs` | All tool implementations |
| `ui/shell/*.py` | `src/ui/shell/*.rs` | TUI shell |
| `ui/print/*.py` | `src/ui/print/*.rs` | Print mode |
| `ui/acp/*.py` | `src/ui/acp/*.rs` | ACP UI stub |
| `ui/theme.py` | `src/ui/theme.rs` | Color definitions |
| `background/*.py` | `src/background/*.rs` | Background tasks |
| `subagents/*.py` | `src/subagents/*.rs` | Subagents |
| `notifications/*.py` | `src/notifications/*.rs` | Notifications |
| `approval_runtime/*.py` | `src/approval_runtime/*.rs` | Approval runtime |
| `hooks/*.py` | `src/hooks/*.rs` | Hooks engine |
| `auth/*.py` | `src/auth/*.rs` | OAuth + platforms |
| `web/*.py` | `src/web/*.rs` | Axum web server |
| `vis/*.py` | `src/vis/*.rs` | Vis server |
| `acp/*.py` | `src/acp/*.rs` | ACP server |
| `utils/*.py` | `src/utils/*.rs` | Utilities |
| `metadata.py` | `src/metadata.rs` | Metadata store |
| `session_state.py` | `src/session_state.rs` | Session state |
| `session_fork.py` | `src/session_fork.rs` | Fork logic |
| `share.py` | `src/share.rs` | Share directory |
| `constant.py` | `src/constant.rs` | Constants |
| `exception.py` | `src/error.rs` | Error types |
| `cli/*.py` | `src/cli/*.rs` | Clap CLI |

## Phase Roadmap
1. **Foundation**: `error`, `constant`, `share`, `config`, `metadata`, `session_state`, `session`, `wire/types`, `llm`
2. **Soul Core**: `soul/agent`, `soul/context`, `soul/toolset`, `soul/message`, `soul/approval`, `soul/slash`, `soul/compaction`, `soul/kimi_soul`, `soul/mod`
3. **Tools**: `tools/file`, `tools/shell`, `tools/web`, `tools/ask_user`, `tools/plan`, `tools/background`, `tools/think`, `tools/todo`, `tools/dmail`, `tools/display`, `tools/utils`
4. **Runtime Extensions**: `background`, `subagents`, `notifications`, `approval_runtime`, `hooks`, `auth`, `soul/btw`, `soul/denwa_renji`
5. **UI Layer**: `ui/theme`, `ui/print`, `ui/acp`, `ui/shell`
6. **Servers**: `web`, `vis`, `acp`
7. **CLI**: `cli`, `app`
8. **Integration**: wiring, tests, binary build
