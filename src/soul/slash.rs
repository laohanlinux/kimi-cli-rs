use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// A slash command definition.
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub handler: SlashHandler,
}

impl std::fmt::Debug for SlashCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlashCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("handler", &"<fn>")
            .finish()
    }
}

/// Handler type for slash commands.
pub type SlashHandler = Box<
    dyn for<'a> Fn(
            &'a mut crate::soul::kimisoul::KimiSoul,
            &'a str,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>
        + Send
        + Sync,
>;

/// Registry of slash commands available in the shell.
#[derive(Debug, Default)]
pub struct SlashCommandRegistry {
    commands: HashMap<String, SlashCommand>,
}

impl SlashCommandRegistry {
    /// Registers a new slash command.
    pub fn register(&mut self, cmd: SlashCommand) {
        self.commands.insert(cmd.name.clone(), cmd);
    }

    /// Looks up a command by name.
    pub fn get(&self, name: &str) -> Option<&SlashCommand> {
        self.commands.get(name)
    }

    /// Returns all registered commands.
    pub fn list(&self) -> Vec<&SlashCommand> {
        self.commands.values().collect()
    }

    /// Consumes the registry and returns all commands.
    pub fn into_commands(self) -> Vec<std::sync::Arc<SlashCommand>> {
        self.commands.into_values().map(std::sync::Arc::new).collect()
    }
}

/// Initializes the default slash command registry.
#[tracing::instrument(level = "debug")]
pub fn default_registry() -> SlashCommandRegistry {
    let mut registry = SlashCommandRegistry::default();

    registry.register(SlashCommand {
        name: "init".into(),
        description: "Analyze the codebase and generate an AGENTS.md file".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_init(soul, args))),
    });

    registry.register(SlashCommand {
        name: "compact".into(),
        description: "Compact the context (optionally with a custom focus)".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_compact(soul, args))),
    });

    registry.register(SlashCommand {
        name: "clear".into(),
        description: "Clear the context".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_clear(soul, args))),
    });

    registry.register(SlashCommand {
        name: "yolo".into(),
        description: "Toggle YOLO mode (auto-approve all actions)".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_yolo(soul, args))),
    });

    registry.register(SlashCommand {
        name: "plan".into(),
        description: "Toggle plan mode. Usage: /plan [on|off|view|clear]".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_plan(soul, args))),
    });

    registry.register(SlashCommand {
        name: "add-dir".into(),
        description: "Add a directory to the workspace".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_add_dir(soul, args))),
    });

    registry.register(SlashCommand {
        name: "export".into(),
        description: "Export current session context to a markdown file".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_export(soul, args))),
    });

    registry.register(SlashCommand {
        name: "import".into(),
        description: "Import context from a file or session ID".into(),
        handler: Box::new(|soul, args| Box::pin(cmd_import(soul, args))),
    });

    registry
}

async fn cmd_init(soul: &mut crate::soul::kimisoul::KimiSoul, _args: &str) {
    let work_dir = &soul.runtime.session.work_dir;
    tracing::info!("Analyzing codebase in {}", work_dir.display());

    let mut project_type = "unknown";
    let mut languages = Vec::new();

    if work_dir.join("Cargo.toml").exists() {
        project_type = "rust";
        languages.push("Rust");
    }
    if work_dir.join("package.json").exists() {
        project_type = "node";
        languages.push("JavaScript/TypeScript");
    }
    if work_dir.join("pyproject.toml").exists() || work_dir.join("setup.py").exists() {
        project_type = "python";
        languages.push("Python");
    }
    if work_dir.join("go.mod").exists() {
        project_type = "go";
        languages.push("Go");
    }

    let lang_text = if languages.is_empty() {
        "Unknown".into()
    } else {
        languages.join(" / ")
    };

    let agents_md = format!(
        r#"# Agent Guide for {}

## Project Type
{}

## Primary Languages
{}

## Key Conventions
- Follow existing code style in the repository.
- Prefer editing existing files over creating new ones when possible.
- Run tests before declaring a task complete.
- Ask for clarification when requirements are ambiguous.

## Build / Test Commands
<!-- Add your project's build and test commands here -->

## Notes
<!-- Add any project-specific notes here -->
"#,
        work_dir.file_name().unwrap_or(work_dir.as_os_str()).to_string_lossy(),
        project_type,
        lang_text,
    );

    let path = work_dir.join("AGENTS.md");
    match tokio::fs::write(&path, agents_md).await {
        Ok(_) => tracing::info!("Generated AGENTS.md at {}", path.display()),
        Err(e) => tracing::warn!("Failed to write AGENTS.md: {}", e),
    }
}

async fn cmd_compact(soul: &mut crate::soul::kimisoul::KimiSoul, args: &str) {
    tracing::info!("Running /compact");
    soul.compact_context(args.trim()).await;
}

async fn cmd_clear(soul: &mut crate::soul::kimisoul::KimiSoul, _args: &str) {
    tracing::info!("Running /clear");
    soul.clear_context().await;
}

async fn cmd_yolo(soul: &mut crate::soul::kimisoul::KimiSoul, _args: &str) {
    let runtime = &mut soul.runtime;
    if runtime.approval.yolo {
        runtime.approval.yolo = false;
        tracing::info!("YOLO mode disabled");
    } else {
        runtime.approval.yolo = true;
        tracing::info!("YOLO mode enabled");
    }
}

async fn cmd_plan(soul: &mut crate::soul::kimisoul::KimiSoul, args: &str) {
    let subcmd = args.trim().to_lowercase();
    match subcmd.as_str() {
        "on" => soul.set_plan_mode(true).await,
        "off" => soul.set_plan_mode(false).await,
        "view" => {
            if let Some(content) = soul.read_current_plan() {
                tracing::info!("Current plan:\n{}", content);
            } else {
                tracing::info!("No active plan.");
            }
        }
        "clear" => {
            soul.clear_current_plan();
            tracing::info!("Plan cleared.");
        }
        _ => {
            let new_state = !soul.plan_mode;
            soul.set_plan_mode(new_state).await;
        }
    }
}

async fn cmd_add_dir(soul: &mut crate::soul::kimisoul::KimiSoul, args: &str) {
    let path = args.trim();
    if path.is_empty() {
        tracing::info!("No directory provided. Usage: /add-dir <path>");
        return;
    }
    let path = std::path::PathBuf::from(path);
    if !path.exists() {
        tracing::warn!("Directory does not exist: {}", path.display());
        return;
    }
    if !path.is_dir() {
        tracing::warn!("Not a directory: {}", path.display());
        return;
    }
    soul.runtime.session.state.additional_dirs.push(path.to_string_lossy().to_string());
    let _ = soul.runtime.session.save_state();
}

async fn cmd_export(soul: &mut crate::soul::kimisoul::KimiSoul, args: &str) {
    let target = args.trim();
    let target_path = if target.is_empty() {
        let share_dir = crate::share::get_share_dir().unwrap_or_else(|_| std::env::temp_dir());
        share_dir.join("exports").join(format!("{}.md", soul.runtime.session.id))
    } else {
        std::path::PathBuf::from(target)
    };
    if let Some(parent) = target_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::copy(&soul.runtime.session.context_file, &target_path) {
        Ok(_) => tracing::info!("Exported context to {}", target_path.display()),
        Err(e) => tracing::warn!("Failed to export context: {}", e),
    }
}

async fn cmd_import(soul: &mut crate::soul::kimisoul::KimiSoul, args: &str) {
    let target = args.trim();
    if target.is_empty() {
        tracing::info!("No source provided. Usage: /import <path|session_id>");
        return;
    }
    let target_path = std::path::PathBuf::from(target);
    if target_path.is_file() {
        match tokio::fs::read_to_string(&target_path).await {
            Ok(content) => {
                let mut file = match tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&soul.runtime.session.context_file)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!("Failed to open context file: {}", e);
                        return;
                    }
                };
                use tokio::io::AsyncWriteExt;
                if let Err(e) = file.write_all(content.as_bytes()).await {
                    tracing::warn!("Failed to import context: {}", e);
                } else {
                    tracing::info!("Imported {} into context", target_path.display());
                }
            }
            Err(e) => tracing::warn!("Failed to read import file: {}", e),
        }
    } else {
        // Try to resolve as session ID in current work dir.
        let work_dir = soul.runtime.session.work_dir.clone();
        let sessions_dir = crate::metadata::WorkDirMeta {
            path: work_dir.to_string_lossy().to_string(),
            kaos: "local".into(),
            last_session_id: None,
        }
        .sessions_dir();
        let session_file = sessions_dir.join(target).join("context.jsonl");
        if session_file.is_file() {
            match tokio::fs::read_to_string(&session_file).await {
                Ok(content) => {
                    let mut file = match tokio::fs::OpenOptions::new()
                        .append(true)
                        .open(&soul.runtime.session.context_file)
                        .await
                    {
                        Ok(f) => f,
                        Err(e) => {
                            tracing::warn!("Failed to open context file: {}", e);
                            return;
                        }
                    };
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = file.write_all(content.as_bytes()).await {
                        tracing::warn!("Failed to import context: {}", e);
                    } else {
                        tracing::info!("Imported session {} into context", target);
                    }
                }
                Err(e) => tracing::warn!("Failed to read session context: {}", e),
            }
        } else {
            tracing::warn!("Import target not found: {}", target);
        }
    }
}
