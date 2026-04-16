use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Custom exit codes.
pub struct ExitCode;

impl ExitCode {
    pub const SUCCESS: i32 = 0;
    pub const FAILURE: i32 = 1;
    pub const RETRYABLE: i32 = 75;
}

/// Reload configuration exception.
#[derive(Debug, Clone)]
pub struct Reload {
    pub session_id: Option<String>,
    pub prefill_text: Option<String>,
}

/// Switch to web interface exception.
#[derive(Debug, Clone)]
pub struct SwitchToWeb {
    pub session_id: Option<String>,
}

/// Switch to vis interface exception.
#[derive(Debug, Clone)]
pub struct SwitchToVis {
    pub session_id: Option<String>,
}

/// UI mode options.
pub type UiMode = &'static str;
pub const UI_MODE_SHELL: UiMode = "shell";
pub const UI_MODE_PRINT: UiMode = "print";
pub const UI_MODE_ACP: UiMode = "acp";
pub const UI_MODE_WIRE: UiMode = "wire";

/// Input/output format options.
pub type Format = &'static str;
pub const FORMAT_TEXT: Format = "text";
pub const FORMAT_STREAM_JSON: Format = "stream-json";

/// Kimi CLI agent.
#[derive(Parser, Debug)]
#[command(name = "kimi", about = "Kimi, your next CLI agent.", version)]
pub struct Cli {
    /// Print verbose information.
    #[arg(long)]
    pub verbose: bool,

    /// Enable debug logging.
    #[arg(long)]
    pub debug: bool,

    /// Path to configuration file.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Model name to use.
    #[arg(short, long)]
    pub model: Option<String>,

    /// Enable thinking mode.
    #[arg(long)]
    pub thinking: bool,

    /// Auto-approve all actions (YOLO mode).
    #[arg(long)]
    pub yolo: bool,

    /// Enable plan mode.
    #[arg(long)]
    pub plan: bool,

    /// Custom skills directories.
    #[arg(long = "skills-dir", value_name = "DIR")]
    pub skills_dirs: Vec<PathBuf>,

    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the interactive shell (default).
    Shell {
        /// Optional initial command.
        command: Option<String>,
    },
    /// Run a single command in print mode.
    #[command(visible_alias = "run")]
    Print {
        /// Command to execute.
        command: Vec<String>,
    },
    /// Start the ACP server.
    Acp,
    /// Start the web server.
    Web,
    /// Start the vis server.
    Vis,
    /// List sessions.
    Sessions {
        /// Show archived sessions.
        #[arg(long)]
        archived: bool,
    },
    /// Export a session.
    Export {
        /// Session ID to export.
        session_id: String,
    },
    /// Import context into a session.
    Import {
        /// File path or session ID to import.
        target: String,
    },
    /// Manage MCP server configurations.
    #[command(subcommand)]
    Mcp(crate::mcp::cli::McpCommand),
}

/// Parses CLI arguments and returns the parsed structure.
pub fn parse() -> Cli {
    Cli::parse()
}

/// Strips the trailing session ID suffix from a title.
pub fn strip_session_id_suffix(title: &str, session_id: &str) -> String {
    let suffix = format!(" ({session_id})");
    if title.ends_with(&suffix) {
        title[..title.len() - suffix.len()].to_string()
    } else {
        title.to_string()
    }
}
