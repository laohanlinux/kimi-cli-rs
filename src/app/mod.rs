use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Outcome of running the interactive shell.
#[derive(Debug, Clone)]
pub enum ShellOutcome {
    /// User exited normally.
    Exit,
    /// Reload with an optional session ID and prefill text.
    Reload { session_id: Option<String>, prefill_text: Option<String> },
    /// Switch to the web interface.
    SwitchToWeb { session_id: Option<String> },
    /// Switch to the vis interface.
    SwitchToVis { session_id: Option<String> },
}

/// Main application orchestrator.
pub struct KimiCLI {
    soul: crate::soul::kimisoul::KimiSoul,
    runtime: crate::soul::agent::Runtime,
    env_overrides: HashMap<String, String>,
}

impl KimiCLI {
    /// Returns the environment variable overrides.
    pub fn env_overrides(&self) -> &HashMap<String, String> {
        &self.env_overrides
    }

    /// Factory that bootstraps the full application stack.
    #[tracing::instrument(level = "info", skip_all)]
    pub async fn create(
        session: crate::session::Session,
        config: Option<crate::config::Config>,
        model_name: Option<&str>,
        thinking: Option<bool>,
        yolo: bool,
        plan_mode: bool,
        resumed: bool,
        agent_file: Option<&Path>,
        skills_dirs: Option<Vec<PathBuf>>,
    ) -> crate::error::Result<Self> {
        let config = match config {
            Some(c) => c,
            None => crate::config::load_config(None)?,
        };

        let oauth = crate::auth::oauth::OAuthManager::default();

        let (model, mut provider) = if let Some(name) = model_name {
            config
                .models
                .get(name)
                .and_then(|m| config.providers.get(&m.provider).map(|p| (m.clone(), p.clone())))
        } else if !config.default_model.is_empty() {
            config
                .models
                .get(&config.default_model)
                .and_then(|m| config.providers.get(&m.provider).map(|p| (m.clone(), p.clone())))
        } else {
            None
        }
        .unwrap_or_else(|| {
            (
                crate::config::LlmModel {
                    provider: "kimi".into(),
                    model: "kimi-k2.5".into(),
                    max_context_size: 128_000,
                    capabilities: None,
                },
                crate::config::LlmProvider {
                    r#type: "kimi".into(),
                    base_url: "https://api.moonshot.cn".into(),
                    api_key: secrecy::SecretString::new("".into()),
                    env: None,
                    custom_headers: None,
                    oauth: None,
                },
            )
        });

        if let Some(resolved_key) = oauth.resolve_api_key(&provider.api_key, provider.oauth.as_ref()).await {
            provider.api_key = resolved_key;
        }

        let thinking = thinking.unwrap_or(config.default_thinking);
        let yolo = yolo || config.default_yolo;
        let _plan_mode = if resumed { plan_mode } else { plan_mode || config.default_plan_mode };

        let llm = crate::llm::create_llm(&provider, &model, Some(thinking), Some(&session.id))
            .await?;

        let runtime = crate::soul::agent::Runtime::create(
            config,
            oauth,
            llm,
            session.clone(),
            yolo,
            skills_dirs,
        )
        .await?;

        let global_mcp_config = crate::mcp::cli::load_mcp_config();
        let mcp_configs = if global_mcp_config.servers.is_empty() {
            vec![]
        } else {
            vec![global_mcp_config]
        };

        let default_agent = crate::agentspec::default_agent_file();
        let agent = crate::soul::agent::load_agent(
            agent_file.unwrap_or(&default_agent),
            &runtime,
            mcp_configs,
            false,
        )
        .await?;

        let mut context = crate::soul::context::Context::new(session.context_file.clone());
        context.restore().await?;

        let soul = crate::soul::kimisoul::KimiSoul::new(agent, context, runtime.clone());

        Ok(KimiCLI {
            soul,
            runtime,
            env_overrides: HashMap::new(),
        })
    }

    pub fn soul(&self) -> &crate::soul::kimisoul::KimiSoul {
        &self.soul
    }

    pub fn session(&self) -> &crate::session::Session {
        &self.runtime.session
    }

    /// Runs a single turn and yields wire messages.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(
        &mut self,
        user_input: Vec<crate::soul::message::ContentPart>,
    ) -> crate::error::Result<crate::soul::TurnOutcome> {
        self.run_with_wire(user_input, |_wire| Box::pin(async {}), None)
            .await
    }

    /// Runs a single turn with a custom wire UI loop.
    ///
    /// When `cancel` is `Some(receiver)`, [`KimiSoul::run`] cooperates at step boundaries and
    /// while waiting for tool approval. Use a [`tokio::sync::watch::Sender`] to request cancel.
    #[tracing::instrument(level = "info", skip(self, ui_loop_fn, cancel))]
    pub async fn run_with_wire(
        &mut self,
        user_input: Vec<crate::soul::message::ContentPart>,
        ui_loop_fn: impl FnOnce(crate::wire::Wire) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
        cancel: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> crate::error::Result<crate::soul::TurnOutcome> {
        let cancel_rx = cancel.unwrap_or_else(|| tokio::sync::watch::channel(false).1);
        crate::soul::run_soul(
            &mut self.soul,
            user_input,
            ui_loop_fn,
            cancel_rx,
            &self.runtime,
        )
        .await
    }

    /// Runs a single turn in print mode and prints the assistant response.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run_print(
        &mut self,
        user_input: Vec<crate::soul::message::ContentPart>,
        verbose: bool,
    ) -> crate::error::Result<()> {
        let mut ui = crate::ui::print::PrintUi::new(verbose);
        let outcome = ui.run(self, user_input).await?;
        if let Some(msg) = outcome.final_message {
            let text = msg.extract_text("");
            if !text.is_empty() {
                println!("{}", text);
            }
        }
        Ok(())
    }

    /// Runs the ACP server.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run_acp(self) -> crate::error::Result<()> {
        let server = crate::acp::AcpServer::new(0, self.soul, self.runtime);
        server.serve().await
    }

    /// Runs the wire stdio server.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run_wire_stdio(self) -> crate::error::Result<()> {
        let server = crate::wire::server::WireServer::new(self.soul, self.runtime);
        server.serve().await
    }

    /// Runs the interactive shell UI.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run_shell(
        mut self,
        command: Option<&str>,
        prefill_text: Option<&str>,
    ) -> crate::error::Result<ShellOutcome> {
        if let Some(cmd) = command {
            let parts = vec![crate::soul::message::ContentPart::Text { text: cmd.to_string() }];
            let _outcome = self.run(parts).await?;
            return Ok(ShellOutcome::Exit);
        }

        let mut ui = crate::ui::shell::ShellUi::default();
        let outcome = ui.run(self, prefill_text).await?;
        Ok(outcome)
    }

    pub fn shutdown_background_tasks(&self) {
        let manager = self.runtime.background_tasks.clone();
        tokio::spawn(async move {
            let tasks = manager.list(false).await;
            for task in tasks {
                let _ = manager.stop(&task.id).await;
            }
            tracing::info!("background tasks shut down");
        });
    }
}
