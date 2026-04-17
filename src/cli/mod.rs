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

#[derive(Subcommand, Debug, Clone)]
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
    /// Show system and configuration information.
    Info,
    /// Manage plugins.
    Plugin {
        /// List installed plugins.
        #[arg(long)]
        list: bool,
    },
    /// Toad mode easter egg.
    Toad,
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

/// Runs the CLI application.
pub async fn run(args: &Cli) -> crate::error::Result<()> {
    let config_path = args.config.as_deref();
    let config = crate::config::load_config(config_path)?;

    let share_dir = crate::share::get_share_dir()?;
    let _logs_dir = share_dir.join("logs");
    std::fs::create_dir_all(&_logs_dir).ok();

    match &args.command {
        Some(Command::Shell { command }) => {
            run_shell_mode(args, &config, command.clone()).await
        }
        Some(Command::Print { command }) => {
            run_print_mode(args, &config, command).await
        }
        Some(Command::Acp) => {
            let work_dir = std::env::current_dir()?;
            let session = match crate::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => crate::session::create(work_dir.clone(), None, None).await?,
            };
            let app = crate::app::KimiCLI::create(
                session,
                Some(config.clone()),
                args.model.as_deref(),
                Some(args.thinking),
                args.yolo,
                args.plan,
                false,
                None,
                Some(args.skills_dirs.clone()),
            )
            .await?;
            app.run_acp().await
        }
        Some(Command::Web) => {
            let server = crate::web::WebServer::new(0);
            server.serve().await
        }
        Some(Command::Vis) => {
            let server = crate::vis::VisServer::new(0);
            server.serve().await
        }
        Some(Command::Sessions { archived }) => {
            let work_dir = std::env::current_dir()?;
            let sessions = crate::session::list(work_dir.clone()).await;
            for s in sessions {
                if !archived && s.state.archived {
                    continue;
                }
                let mark = if s.state.archived { "[archived] " } else { "" };
                println!("{}{} {}", mark, s.id, s.title);
            }
            Ok(())
        }
        Some(Command::Export { session_id }) => {
            let work_dir = std::env::current_dir()?;
            let session = match crate::session::find(work_dir.clone(), session_id).await {
                Some(s) => s,
                None => {
                    eprintln!("Session {} not found", session_id);
                    std::process::exit(1);
                }
            };
            let export_dir = share_dir.join("exports");
            std::fs::create_dir_all(&export_dir)?;
            let export_path = export_dir.join(format!("{session_id}.jsonl"));
            if session.context_file.exists() {
                std::fs::copy(&session.context_file, &export_path)?;
            }
            println!("Exported session {} to {}", session_id, export_path.display());
            Ok(())
        }
        Some(Command::Import { target }) => {
            let work_dir = std::env::current_dir()?;
            let session = match crate::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => crate::session::create(work_dir.clone(), None, None).await?,
            };
            let target_path = std::path::PathBuf::from(target);
            if !target_path.exists() {
                eprintln!("Target file {} not found", target_path.display());
                std::process::exit(1);
            }
            let content = tokio::fs::read_to_string(&target_path).await?;
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&session.context_file)
                .await?;
            use tokio::io::AsyncWriteExt;
            file.write_all(content.as_bytes()).await?;
            println!(
                "Imported {} into session {}",
                target_path.display(),
                session.id
            );
            Ok(())
        }
        Some(Command::Info) => {
            println!("Kimi CLI (Rust port)");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!("Config path: {:?}", config_path);
            println!("Share dir: {}", share_dir.display());
            println!("Working dir: {}", std::env::current_dir()?.display());
            println!("Default model: {}", config.default_model);
            println!("YOLO mode: {}", if args.yolo { "enabled" } else { "disabled" });
            println!("Plan mode: {}", if args.plan { "enabled" } else { "disabled" });
            Ok(())
        }
        Some(Command::Plugin { list }) => {
            let plugins_dir = crate::plugin::get_plugins_dir();
            if *list {
                match std::fs::read_dir(&plugins_dir) {
                    Ok(entries) => {
                        let mut count = 0;
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                println!("{}", path.file_name().unwrap_or_default().to_string_lossy());
                                count += 1;
                            }
                        }
                        if count == 0 {
                            println!("No plugins installed.");
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to read plugins directory: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                println!("Plugins directory: {}", plugins_dir.display());
                println!("Use --list to see installed plugins.");
            }
            Ok(())
        }
        Some(Command::Toad) => {
            println!("🐸 Toad says: Ribbit ribbit!");
            Ok(())
        }
        Some(Command::Mcp(mcp_cmd)) => {
            crate::mcp::cli::run(mcp_cmd.clone()).await
        }
        None => {
            run_shell_mode(args, &config, None).await
        }
    }
}

async fn run_shell_mode(
    args: &Cli,
    config: &crate::config::Config,
    command_override: Option<String>,
) -> crate::error::Result<()> {
    let work_dir = std::env::current_dir()?;
    let mut session = match crate::session::continue_(work_dir.clone()).await {
        Some(s) => s,
        None => crate::session::create(work_dir.clone(), None, None).await?,
    };
    let mut prefill = None::<String>;
    let mut command_override = command_override;
    loop {
        let app = crate::app::KimiCLI::create(
            session.clone(),
            Some(config.clone()),
            args.model.as_deref(),
            Some(args.thinking),
            args.yolo,
            args.plan,
            false,
            None,
            Some(args.skills_dirs.clone()),
        )
        .await?;
        match app
            .run_shell(command_override.as_deref(), prefill.as_deref())
            .await?
        {
            crate::app::ShellOutcome::Exit => break Ok(()),
            crate::app::ShellOutcome::Reload {
                session_id,
                prefill_text,
            } => {
                session = if let Some(id) = session_id {
                    match crate::session::find(work_dir.clone(), &id).await {
                        Some(s) => s,
                        None => crate::session::create(work_dir.clone(), None, None).await?,
                    }
                } else {
                    crate::session::create(work_dir.clone(), None, None).await?
                };
                prefill = prefill_text;
                command_override = None;
            }
            crate::app::ShellOutcome::SwitchToWeb { session_id: _ } => {
                let server = crate::web::WebServer::new(0);
                server.serve().await?;
                break Ok(());
            }
            crate::app::ShellOutcome::SwitchToVis { session_id: _ } => {
                let server = crate::vis::VisServer::new(0);
                server.serve().await?;
                break Ok(());
            }
        }
    }
}

async fn run_print_mode(
    args: &Cli,
    config: &crate::config::Config,
    command: &[String],
) -> crate::error::Result<()> {
    let work_dir = std::env::current_dir()?;
    let session = match crate::session::continue_(work_dir.clone()).await {
        Some(s) => s,
        None => crate::session::create(work_dir.clone(), None, None).await?,
    };
    let mut app = crate::app::KimiCLI::create(
        session,
        Some(config.clone()),
        args.model.as_deref(),
        Some(args.thinking),
        args.yolo,
        args.plan,
        false,
        None,
        Some(args.skills_dirs.clone()),
    )
    .await?;
    let text = command.join(" ");
    if !text.is_empty() {
        let parts = vec![crate::soul::message::ContentPart::Text { text }];
        app.run_print(parts, args.verbose).await?;
    } else {
        tracing::warn!("No command provided for print mode");
    }
    Ok(())
}
