/// Main entrypoint for the Kimi CLI.
#[tokio::main]
#[tracing::instrument(level = "info")]
async fn main() {
    if let Err(e) = _main().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn _main() -> Result<(), kimi_cli_rs::error::KimiCliError> {
    let args = kimi_cli_rs::cli::parse();

    // Initialize tracing subscriber.
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    if args.debug {
        tracing::info!("Debug logging enabled");
    }

    // Load configuration.
    let config_path = args.config.as_deref();
    let config = kimi_cli_rs::config::load_config(config_path)?;

    // Resolve share dir and ensure logs directory exists.
    let share_dir = kimi_cli_rs::share::get_share_dir()?;
    let _logs_dir = share_dir.join("logs");
    std::fs::create_dir_all(&_logs_dir).ok();

    // Dispatch to subcommand or default shell.
    match args.command {
        Some(kimi_cli_rs::cli::Command::Shell { command }) => {
            let work_dir = std::env::current_dir()?;
            let mut session = match kimi_cli_rs::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => kimi_cli_rs::session::create(work_dir.clone(), None, None).await?,
            };
            let mut prefill = None::<String>;
            let mut command_override = command.clone();
            loop {
                let mut app = kimi_cli_rs::app::KimiCLI::create(
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
                    kimi_cli_rs::app::ShellOutcome::Exit => break,
                    kimi_cli_rs::app::ShellOutcome::Reload {
                        session_id,
                        prefill_text,
                    } => {
                        session = if let Some(id) = session_id {
                            match kimi_cli_rs::session::find(work_dir.clone(), &id).await {
                                Some(s) => s,
                                None => {
                                    kimi_cli_rs::session::create(work_dir.clone(), None, None)
                                        .await?
                                }
                            }
                        } else {
                            kimi_cli_rs::session::create(work_dir.clone(), None, None).await?
                        };
                        prefill = prefill_text;
                        command_override = None;
                    }
                    kimi_cli_rs::app::ShellOutcome::SwitchToWeb { session_id: _ } => {
                        let server = kimi_cli_rs::web::WebServer::new(0);
                        server.serve().await?;
                        break;
                    }
                    kimi_cli_rs::app::ShellOutcome::SwitchToVis { session_id: _ } => {
                        let server = kimi_cli_rs::vis::VisServer::new(0);
                        server.serve().await?;
                        break;
                    }
                }
            }
        }
        Some(kimi_cli_rs::cli::Command::Print { command }) => {
            let work_dir = std::env::current_dir()?;
            let session = match kimi_cli_rs::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => kimi_cli_rs::session::create(work_dir.clone(), None, None).await?,
            };
            let mut app = kimi_cli_rs::app::KimiCLI::create(
                session,
                Some(config),
                args.model.as_deref(),
                Some(args.thinking),
                args.yolo,
                args.plan,
                false,
                None,
                Some(args.skills_dirs),
            )
            .await?;
            let text = command.join(" ");
            if !text.is_empty() {
                let parts = vec![kimi_cli_rs::soul::message::ContentPart::Text { text }];
                let outcome = app.run(parts).await?;
                if let Some(msg) = outcome.final_message {
                    println!("{}", msg.extract_text(""));
                }
            } else {
                tracing::warn!("No command provided for print mode");
            }
        }
        Some(kimi_cli_rs::cli::Command::Acp) => {
            let server = kimi_cli_rs::acp::AcpServer::new(0);
            server.serve().await?;
        }
        Some(kimi_cli_rs::cli::Command::Web) => {
            let server = kimi_cli_rs::web::WebServer::new(0);
            server.serve().await?;
        }
        Some(kimi_cli_rs::cli::Command::Vis) => {
            let server = kimi_cli_rs::vis::VisServer::new(0);
            server.serve().await?;
        }
        Some(kimi_cli_rs::cli::Command::Sessions { archived }) => {
            let work_dir = std::env::current_dir()?;
            let sessions = kimi_cli_rs::session::list(work_dir.clone()).await;
            for s in sessions {
                if !archived && s.state.archived {
                    continue;
                }
                let mark = if s.state.archived { "[archived] " } else { "" };
                println!("{}{} {}", mark, s.id, s.title);
            }
        }
        Some(kimi_cli_rs::cli::Command::Export { session_id }) => {
            let work_dir = std::env::current_dir()?;
            let session = match kimi_cli_rs::session::find(work_dir.clone(), &session_id).await {
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
        }
        Some(kimi_cli_rs::cli::Command::Import { target }) => {
            let work_dir = std::env::current_dir()?;
            let session = match kimi_cli_rs::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => kimi_cli_rs::session::create(work_dir.clone(), None, None).await?,
            };
            let target_path = std::path::PathBuf::from(&target);
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
        }
        None => {
            // Default to shell mode.
            let work_dir = std::env::current_dir()?;
            let mut session = match kimi_cli_rs::session::continue_(work_dir.clone()).await {
                Some(s) => s,
                None => kimi_cli_rs::session::create(work_dir.clone(), None, None).await?,
            };
            let mut prefill = None::<String>;
            let mut command_override: Option<String> = None;
            loop {
                let mut app = kimi_cli_rs::app::KimiCLI::create(
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
                    kimi_cli_rs::app::ShellOutcome::Exit => break,
                    kimi_cli_rs::app::ShellOutcome::Reload {
                        session_id,
                        prefill_text,
                    } => {
                        session = if let Some(id) = session_id {
                            match kimi_cli_rs::session::find(work_dir.clone(), &id).await {
                                Some(s) => s,
                                None => {
                                    kimi_cli_rs::session::create(work_dir.clone(), None, None)
                                        .await?
                                }
                            }
                        } else {
                            kimi_cli_rs::session::create(work_dir.clone(), None, None).await?
                        };
                        prefill = prefill_text;
                        command_override = None;
                    }
                    kimi_cli_rs::app::ShellOutcome::SwitchToWeb { session_id: _ } => {
                        let server = kimi_cli_rs::web::WebServer::new(0);
                        server.serve().await?;
                        break;
                    }
                    kimi_cli_rs::app::ShellOutcome::SwitchToVis { session_id: _ } => {
                        let server = kimi_cli_rs::vis::VisServer::new(0);
                        server.serve().await?;
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
