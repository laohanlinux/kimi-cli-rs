# Phase 7: CLI and Application Orchestrator Translation Plan

## Objective
Translate the Python `cli/*.py` and `app.py` modules into a `clap`-based CLI parser and an async application bootstrapper (`KimiCLI`).

## 7.1 `src/cli/mod.rs`

**Strategy:** Use `clap` derive macros for argument parsing and define custom control-flow exceptions as plain Rust structs.

### Custom Control Flow Types
```rust
pub struct Reload {
    pub session_id: Option<String>,
    pub prefill_text: Option<String>,
}

pub struct SwitchToWeb {
    pub session_id: Option<String>,
}

pub struct SwitchToVis {
    pub session_id: Option<String>,
}
```

### CLI Definition
```rust
#[derive(Parser, Debug)]
#[command(name = "kimi", about = "Kimi, your next CLI agent.", version)]
pub struct Cli {
    #[arg(long)]
    pub verbose: bool,
    #[arg(long)]
    pub debug: bool,
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(long)]
    pub thinking: bool,
    #[arg(long)]
    pub yolo: bool,
    #[arg(long)]
    pub plan: bool,
    #[arg(long = "skills-dir", value_name = "DIR")]
    pub skills_dirs: Vec<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Shell { command: Option<String> },
    #[command(visible_alias = "run")]
    Print { command: Vec<String> },
    Acp,
    Web,
    Vis,
    Sessions { #[arg(long)] archived: bool },
    Export { session_id: String },
    Import { target: String },
}
```

### Utility Functions
- `parse()` → delegates to `Cli::parse()`.
- `strip_session_id_suffix(title, session_id)` → removes trailing ` (id)` from titles.

## 7.2 `src/app/mod.rs`

**Strategy:** `KimiCLI` is the main application orchestrator. It creates the `Runtime`, loads the `Agent`, restores `Context`, and exposes `run()` and `run_shell()`.

### Struct
```rust
pub struct KimiCLI {
    soul: crate::soul::kimisoul::KimiSoul,
    runtime: crate::soul::agent::Runtime,
    env_overrides: HashMap<String, String>,
}
```

### Factory (`KimiCLI::create`)
1. Load or use provided `Config`.
2. Resolve model/provider (CLI override > config default > built-in Kimi default).
3. Build `Llm` via `crate::llm::create_llm()`.
4. Build `Runtime` with `OAuthManager`, session, and skills directories.
5. Load `Agent` from `AGENTS.md` (or provided agent file).
6. Restore `Context` from the session `context.jsonl` file.
7. Construct `KimiSoul` from agent + context + runtime.

### Methods
- `soul()` → `&KimiSoul`
- `session()` → `&Session` (via runtime)
- `run(user_input)` → calls `run_soul()` with a no-op UI loop and returns `TurnOutcome`.
- `run_shell(command, prefill_text)` → if a command string is provided, runs a single turn directly; otherwise instantiates `ShellUi` and enters the interactive ratatui event loop.
- `shutdown_background_tasks()` → lists all running background tasks and stops them asynchronously.

## Tracing Strategy for CLI / App
- `KimiCLI::create` → `#[tracing::instrument(level = "info", skip_all)]`
- `KimiCLI::run` and `KimiCLI::run_shell` → `info` level.
- Log resolved model name, thinking flag, yolo mode, and plan mode at startup.
