# Phase 8: Integration and Build Translation Plan

## Objective
Wire all modules together through `lib.rs`, provide a robust `main.rs` entrypoint, configure the Cargo package, and establish unit plus integration test coverage.

## 8.1 `src/lib.rs`

**Strategy:** Re-export every top-level module so that integration tests and external consumers can access the public API.

```rust
pub mod acp;
pub mod agentspec;
pub mod app;
pub mod approval_runtime;
pub mod auth;
pub mod background;
pub mod cli;
pub mod config;
pub mod constant;
pub mod error;
pub mod hooks;
pub mod llm;
pub mod metadata;
pub mod notifications;
pub mod plugin;
pub mod session;
pub mod session_fork;
pub mod session_state;
pub mod share;
pub mod skill;
pub mod soul;
pub mod subagents;
pub mod tools;
pub mod ui;
pub mod utils;
pub mod vis;
pub mod web;
pub mod wire;
```

## 8.2 `src/main.rs`

**Strategy:** Small `tokio::main` wrapper that initializes tracing, parses CLI arguments, loads config, and dispatches to the appropriate subcommand handler.

### Main Flow
1. Initialize `tracing_subscriber` with `EnvFilter` (defaults to `info`).
2. Parse CLI via `kimi_cli_rs::cli::parse()`.
3. Load config from `--config` or default share path.
4. Resolve share dir and ensure `logs/` exists.
5. `match` on `args.command`:
   - `Shell` / default → `session::continue_()` or `session::create()`, then `KimiCLI::create()` → `run_shell()`.
   - `Print` → Same bootstrap, then `KimiCLI::run()` with piped or argument text.
   - `Acp` → `AcpServer::new(0).serve().await`.
   - `Web` → `WebServer::new(0).serve().await`.
   - `Vis` → `VisServer::new(0).serve().await`.
   - `Sessions { archived }` → `session::list()` with archive filter.
   - `Export { session_id }` → Copy session `context.jsonl` to `share/exports/{id}.jsonl`.
   - `Import { target }` → Append target file contents into the current session context file.

### Error Handling
Any error in `_main()` is printed to `stderr` and the process exits with code `1`.

## 8.3 `Cargo.toml`

**Key Configuration:**
- `edition = "2024"`
- Binary target: `src/main.rs`
- Async runtime: `tokio` (full features)
- Web stack: `axum`, `tower-http`, `reqwest`
- TUI: `ratatui`, `crossterm`, `ansi_term`
- Serialization: `serde`, `serde_json`, `serde_yaml`, `toml`
- Error handling: `thiserror`, `color-eyre`
- CLI: `clap` (derive, env, cargo)
- Tracing: `tracing`, `tracing-subscriber`
- Security: `secrecy`
- Utilities: `uuid`, `chrono`, `regex`, `glob`, `md5`, `dunce`, `dirs`, `notify`, `oauth2`, `strum`, `pulldown-cmark`, `similar`, `diffy`, `zip`, `tar`, `flate2`, `sysinfo`, `base64`, `image`, `bytes`, `async-trait`, `minijinja`

**Dev Dependencies:**
- `tokio-test`
- `assert_fs`
- `predicates`

## 8.4 Testing Strategy

### Unit Tests (in-module `#[cfg(test)]`)
Located inside the modules they exercise:
- `config::tests` — save/load roundtrip
- `session_state::tests` — JSON roundtrip
- `tools::shell::tests` — echo and empty command
- `tools::web::tests` — fetch stub and unconfigured search
- `web::api::tests` — `WebSession` serialization and title response
- `ui::shell::tests` — `ShellUi` default and history push

**Total unit tests:** 43

### Integration Tests (`tests/`)
- `tests/session_integration.rs`
  - `session_create_find_list_delete_roundtrip`
  - `session_continue_returns_latest`
- `tests/web_api_integration.rs`
  - `healthz_returns_ok`
  - `session_crud_roundtrip`
  - `git_diff_for_non_git_repo`

**Test Isolation:**
- Integration tests use `uuid::Uuid::new_v4()` to create temp share and work directories.
- `kimi_cli_rs::share::set_test_share_dir(dir)` provides a thread-local override to avoid mutating the global `KIMI_SHARE_DIR` env var, preventing race conditions during parallel test execution.

## 8.5 Build Verification

Run the following to verify the port:
```bash
cargo test
cargo build --release
cargo run -- --help
```

Expected results:
- 43 unit tests pass
- 5 integration tests pass
- Build completes with only expected unused-field/method warnings from stub placeholders
