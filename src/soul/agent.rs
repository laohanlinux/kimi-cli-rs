use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Builtin system prompt arguments.
#[derive(Debug, Clone)]
#[allow(non_snake_case)]
pub struct BuiltinSystemPromptArgs {
    pub KIMI_NOW: String,
    pub KIMI_WORK_DIR: PathBuf,
    pub KIMI_WORK_DIR_LS: String,
    pub KIMI_AGENTS_MD: String,
    pub KIMI_SKILLS: String,
    pub KIMI_ADDITIONAL_DIRS_INFO: String,
    pub KIMI_OS: String,
    pub KIMI_SHELL: String,
}

const AGENTS_MD_MAX_BYTES: usize = 32 * 1024; // 32 KiB

async fn _find_project_root(work_dir: &Path) -> PathBuf {
    let mut current = work_dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return current;
        }
        let parent = current.parent().map(|p| p.to_path_buf());
        match parent {
            Some(p) if p != current => current = p,
            _ => return work_dir.to_path_buf(),
        }
    }
}

async fn _dirs_root_to_leaf(work_dir: &Path, project_root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut current = work_dir.to_path_buf();
    loop {
        dirs.push(current.clone());
        if current == project_root {
            break;
        }
        let parent = current.parent().map(|p| p.to_path_buf());
        match parent {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    dirs.reverse();
    dirs
}

/// Discovers and merges AGENTS.md files from project root down to work_dir.
#[tracing::instrument(level = "debug")]
pub async fn load_agents_md(work_dir: &Path) -> Option<String> {
    let project_root = _find_project_root(work_dir).await;
    let dirs = _dirs_root_to_leaf(work_dir, &project_root).await;

    let mut discovered: Vec<(PathBuf, String)> = Vec::new();
    for d in &dirs {
        let kimi_path = d.join(".kimi").join("AGENTS.md");
        let root_candidates = [d.join("AGENTS.md"), d.join("agents.md")];

        let mut candidates: Vec<PathBuf> = Vec::new();
        if kimi_path.is_file() {
            candidates.push(kimi_path);
        }
        for rc in &root_candidates {
            if rc.is_file() {
                candidates.push(rc.clone());
                break;
            }
        }

        for path in candidates {
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    let content = content.trim().to_string();
                    if !content.is_empty() {
                        discovered.push((path.clone(), content));
                        tracing::info!("Loaded agents.md: {}", path.display());
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read agents.md {}: {}", path.display(), e);
                }
            }
        }
    }

    if discovered.is_empty() {
        tracing::info!(
            "No AGENTS.md found from {} to {}",
            project_root.display(),
            work_dir.display()
        );
        return None;
    }

    let mut remaining = AGENTS_MD_MAX_BYTES;
    let mut budgeted: Vec<(PathBuf, String)> = vec![(PathBuf::new(), String::new()); discovered.len()];
    for i in (0..discovered.len()).rev() {
        let (path, content) = &discovered[i];
        let annotation = format!("<!-- From: {} -->\n", path.display());
        let separator_cost = if i < discovered.len() - 1 { 2 } else { 0 }; // len(b"\n\n")
        let overhead = annotation.len() + separator_cost;
        if remaining <= overhead {
            budgeted[i] = (path.clone(), String::new());
            remaining = 0;
            continue;
        }
        remaining -= overhead;
        let encoded = content.as_bytes();
        let mut truncated = content.clone();
        if encoded.len() > remaining {
            truncated = String::from_utf8_lossy(&encoded[..remaining]).trim().to_string();
            tracing::warn!("AGENTS.md truncated due to size limit: {}", path.display());
        }
        remaining -= truncated.len();
        budgeted[i] = (path.clone(), truncated);
    }

    let mut parts: Vec<String> = Vec::new();
    for (path, content) in budgeted {
        if !content.is_empty() {
            parts.push(format!("<!-- From: {} -->\n{}", path.display(), content));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Runtime dependency container shared across the system.
#[derive(Debug, Clone)]
pub struct Runtime {
    pub config: crate::config::Config,
    pub oauth: crate::auth::oauth::OAuthManager,
    pub llm: Option<crate::llm::Llm>,
    pub session: crate::session::Session,
    pub builtin_args: BuiltinSystemPromptArgs,
    pub denwa_renji: crate::soul::denwa_renji::DenwaRenji,
    pub approval: crate::soul::approval::Approval,
    pub labor_market: Arc<tokio::sync::RwLock<crate::subagents::labor_market::LaborMarket>>,
    pub environment: crate::utils::environment::Environment,
    pub notifications: crate::notifications::manager::NotificationManager,
    pub background_tasks: crate::background::manager::BackgroundTaskManager,
    pub skills: HashMap<String, crate::skill::Skill>,
    pub additional_dirs: Vec<PathBuf>,
    pub skills_dirs: Vec<PathBuf>,
    pub subagent_store: Option<crate::subagents::store::SubagentStore>,
    pub approval_runtime: Option<Arc<crate::approval_runtime::runtime::ApprovalRuntime>>,
    pub root_wire_hub: Option<Arc<crate::wire::root_hub::RootWireHub>>,
    pub subagent_id: Option<String>,
    pub subagent_type: Option<String>,
    pub role: String,
    pub hook_engine: Option<crate::hooks::engine::HookEngine>,
}

#[cfg(test)]
impl Default for Runtime {
    fn default() -> Self {
        let session = crate::session::Session {
            id: "test".into(),
            work_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            work_dir_meta: crate::metadata::WorkDirMeta {
                path: ".".into(),
                kaos: "local".into(),
                last_session_id: None,
            },
            context_file: PathBuf::from("context.jsonl"),
            wire_file: crate::wire::file::WireFile::new(PathBuf::from("wire.jsonl")),
            state: crate::session_state::SessionState::default(),
            title: "Test".into(),
            updated_at: 0.0,
        };
        Self {
            config: crate::config::Config::default(),
            oauth: crate::auth::oauth::OAuthManager::default(),
            llm: None,
            session: session.clone(),
            builtin_args: BuiltinSystemPromptArgs {
                KIMI_NOW: chrono::Local::now().to_rfc3339(),
                KIMI_WORK_DIR: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                KIMI_WORK_DIR_LS: String::new(),
                KIMI_AGENTS_MD: String::new(),
                KIMI_SKILLS: "No skills found.".into(),
                KIMI_ADDITIONAL_DIRS_INFO: String::new(),
                KIMI_OS: "unknown".into(),
                KIMI_SHELL: "sh".into(),
            },
            denwa_renji: crate::soul::denwa_renji::DenwaRenji::default(),
            approval: crate::soul::approval::Approval::default(),
            labor_market: Arc::new(tokio::sync::RwLock::new(crate::subagents::labor_market::LaborMarket::default())),
            environment: crate::utils::environment::Environment {
                os_kind: "unknown".into(),
                os_arch: "unknown".into(),
                os_version: "unknown".into(),
                shell_name: "sh".into(),
                shell_path: PathBuf::from("/bin/sh"),
            },
            notifications: crate::notifications::manager::NotificationManager::default(),
            background_tasks: crate::background::manager::BackgroundTaskManager::default(),
            skills: HashMap::new(),
            additional_dirs: Vec::new(),
            skills_dirs: Vec::new(),
            subagent_store: Some(crate::subagents::store::SubagentStore::new(&session)),
            approval_runtime: Some(Arc::new(crate::approval_runtime::runtime::ApprovalRuntime::default())),
            root_wire_hub: Some(Arc::new(crate::wire::root_hub::RootWireHub::default())),
            subagent_id: None,
            subagent_type: None,
            role: "root".into(),
            hook_engine: None,
        }
    }
}

impl Runtime {
    /// Factory that bootstraps the runtime.
    #[tracing::instrument(level = "debug")]
    pub async fn create(
        config: crate::config::Config,
        oauth: crate::auth::oauth::OAuthManager,
        llm: Option<crate::llm::Llm>,
        mut session: crate::session::Session,
        yolo: bool,
        skills_dirs: Option<Vec<PathBuf>>,
    ) -> crate::error::Result<Self> {
        let work_dir = session.work_dir.clone();

        let (ls_output, agents_md, environment) = tokio::join!(
            crate::utils::path::list_directory(&work_dir),
            load_agents_md(&work_dir),
            crate::utils::environment::Environment::detect(),
        );

        let skills_roots = crate::skill::resolve_skills_roots(
            &work_dir,
            skills_dirs.as_deref(),
        ).await;
        let skills_roots_canonical: Vec<PathBuf> = skills_roots
            .iter()
            .map(|p| dunce::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();
        let skills = crate::skill::discover_skills_from_roots(&skills_roots).await;
        let skills_by_name = crate::skill::index_skills(&skills)
            .into_iter()
            .map(|(k, v)| (k, v.clone()))
            .collect::<HashMap<_, _>>();
        tracing::info!("Discovered {} skill(s)", skills.len());
        let skills_formatted = if skills.is_empty() {
            "No skills found.".into()
        } else {
            skills
                .iter()
                .map(|skill| {
                    format!(
                        "- {}\n  - Path: {}\n  - Description: {}",
                        skill.name,
                        skill.skill_md_file().display(),
                        skill.description
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let mut additional_dirs: Vec<PathBuf> = Vec::new();
        let mut pruned = false;
        let mut valid_dir_strs: Vec<String> = Vec::new();
        for dir_str in &session.state.additional_dirs {
            let d = dunce::canonicalize(dir_str).unwrap_or_else(|_| PathBuf::from(dir_str));
            if d.is_dir() {
                additional_dirs.push(d);
                valid_dir_strs.push(dir_str.clone());
            } else {
                tracing::warn!(
                    "Additional directory no longer exists, removing from state: {}",
                    dir_str
                );
                pruned = true;
            }
        }
        if pruned {
            session.state.additional_dirs = valid_dir_strs;
            session.save_state()?;
        }

        let mut additional_dirs_info = String::new();
        if !additional_dirs.is_empty() {
            let mut parts: Vec<String> = Vec::new();
            for d in &additional_dirs {
                match crate::utils::path::list_directory(d).await {
                    dir_ls => {
                        parts.push(format!("### `{}`\n\n```\n{}\n```", d.display(), dir_ls));
                    }
                }
            }
            additional_dirs_info = parts.join("\n\n");
        }

        let effective_yolo = yolo || session.state.approval.yolo;
        let saved_actions: Vec<String> = session.state.approval.auto_approve_actions.clone();

        let approval_state_data = crate::session_state::ApprovalStateData {
            yolo: effective_yolo,
            auto_approve_actions: saved_actions,
        };
        let approval_state_arc = Arc::new(std::sync::Mutex::new(approval_state_data));
        let session_state_arc = Arc::new(std::sync::Mutex::new(session.state.clone()));
        let session_dir_for_cb = session.dir();
        let approval_state_for_cb = approval_state_arc.clone();
        let on_change = Arc::new(move || {
            let approval = approval_state_for_cb.lock().unwrap();
            let mut state = session_state_arc.lock().unwrap();
            state.approval.yolo = approval.yolo;
            state.approval.auto_approve_actions = approval.auto_approve_actions.clone();
            let _ = crate::session_state::save_session_state(&*state, &session_dir_for_cb);
        }) as Arc<dyn Fn() + Send + Sync>;

        let approval_state = crate::soul::approval::ApprovalState::new(
            effective_yolo,
            approval_state_arc.lock().unwrap().auto_approve_actions.clone(),
            Some(on_change),
        );

        let notifications = crate::notifications::manager::NotificationManager::new(
            session.context_file.parent().unwrap_or(Path::new(".")),
            config.notifications.clone(),
        );

        let mut background_tasks = crate::background::manager::BackgroundTaskManager::default();

        let runtime = Runtime {
            config: config.clone(),
            oauth,
            llm,
            session,
            builtin_args: BuiltinSystemPromptArgs {
                KIMI_NOW: chrono::Local::now().to_rfc3339(),
                KIMI_WORK_DIR: work_dir.clone(),
                KIMI_WORK_DIR_LS: ls_output,
                KIMI_AGENTS_MD: agents_md.unwrap_or_default(),
                KIMI_SKILLS: skills_formatted,
                KIMI_ADDITIONAL_DIRS_INFO: additional_dirs_info,
                KIMI_OS: environment.os_kind.clone(),
                KIMI_SHELL: format!("{} (`{}`)", environment.shell_name, environment.shell_path.display()),
            },
            denwa_renji: crate::soul::denwa_renji::DenwaRenji::default(),
            approval: crate::soul::approval::Approval::new(false, Some(approval_state), None),
            labor_market: Arc::new(tokio::sync::RwLock::new(crate::subagents::labor_market::LaborMarket::default())),
            environment,
            notifications: notifications.clone(),
            background_tasks: background_tasks.clone(),
            skills: skills_by_name,
            additional_dirs,
            skills_dirs: skills_roots_canonical
                .into_iter()
                .filter(|r| !crate::utils::path::is_within_directory(r, &work_dir))
                .collect(),
            subagent_store: Some(crate::subagents::store::SubagentStore::default()),
            approval_runtime: Some(Arc::new(crate::approval_runtime::runtime::ApprovalRuntime::default())),
            root_wire_hub: Some(Arc::new(crate::wire::root_hub::RootWireHub::default())),
            subagent_id: None,
            subagent_type: None,
            role: "root".into(),
            hook_engine: None,
        };

        if let Some(ref ar) = runtime.approval_runtime {
            if let Some(ref rwh) = runtime.root_wire_hub {
                ar.bind_root_wire_hub(rwh);
            }
        }
        background_tasks.bind_runtime(&runtime);

        Ok(runtime)
    }

    pub fn copy_for_subagent(
        &self,
        agent_id: String,
        subagent_type: String,
        llm_override: Option<crate::llm::Llm>,
    ) -> Self {
        Self {
            config: self.config.clone(),
            oauth: self.oauth.clone(),
            llm: llm_override.or_else(|| self.llm.clone()),
            session: self.session.clone(),
            builtin_args: self.builtin_args.clone(),
            denwa_renji: crate::soul::denwa_renji::DenwaRenji::default(),
            approval: self.approval.share(),
            labor_market: self.labor_market.clone(),
            environment: self.environment.clone(),
            notifications: self.notifications.clone(),
            background_tasks: self.background_tasks.copy_for_role("subagent"),
            skills: self.skills.clone(),
            additional_dirs: self.additional_dirs.clone(),
            skills_dirs: self.skills_dirs.clone(),
            subagent_store: self.subagent_store.clone(),
            approval_runtime: self.approval_runtime.clone(),
            root_wire_hub: self.root_wire_hub.clone(),
            subagent_id: Some(agent_id),
            subagent_type: Some(subagent_type),
            role: "subagent".into(),
            hook_engine: self.hook_engine.clone(),
        }
    }
}

/// An instantiated agent specification.
#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub system_prompt: String,
    pub toolset: crate::soul::toolset::KimiToolset,
    pub runtime: Runtime,
}

/// Loads an agent from the YAML spec file.
#[tracing::instrument(level = "debug")]
pub async fn load_agent(
    agent_file: &Path,
    runtime: &Runtime,
    mcp_configs: Vec<crate::config::McpConfig>,
    start_mcp_loading: bool,
) -> crate::error::Result<Agent> {
    tracing::info!("Loading agent: {}", agent_file.display());
    let agent_spec = crate::agentspec::load_agent_spec(agent_file)?;

    let system_prompt = _load_system_prompt(
        &agent_spec.system_prompt_path,
        &agent_spec.system_prompt_args,
        &runtime.builtin_args,
    )?;

    for (subagent_name, subagent_spec) in &agent_spec.subagents {
        tracing::debug!("Registering builtin subagent type: {}", subagent_name);
        let builtin_spec = crate::agentspec::load_agent_spec(&subagent_spec.path)?;
        let tool_policy = if let Some(ref allowed) = builtin_spec.allowed_tools {
            crate::subagents::labor_market::ToolPolicy::allowlist(allowed.clone())
        } else {
            crate::subagents::labor_market::ToolPolicy::inherit()
        };
        let type_def = crate::subagents::labor_market::AgentTypeDefinition {
            name: subagent_name.clone(),
            description: subagent_spec.description.clone(),
            agent_file: subagent_spec.path.clone(),
            when_to_use: builtin_spec.when_to_use.clone(),
            default_model: builtin_spec.model.clone(),
            tool_policy,
            supports_background: false,
        };
        runtime.labor_market.write().await.add_builtin_type(type_def);
    }

    let mut toolset = crate::soul::toolset::KimiToolset::new();
    let mut tool_deps: HashMap<String, serde_json::Value> = HashMap::new();
    tool_deps.insert("KimiToolset".into(), serde_json::json!(null));
    tool_deps.insert("Runtime".into(), serde_json::json!(null));
    tool_deps.insert("Config".into(), serde_json::json!(null));
    tool_deps.insert("BuiltinSystemPromptArgs".into(), serde_json::json!(null));
    tool_deps.insert("Session".into(), serde_json::json!(null));
    tool_deps.insert("DenwaRenji".into(), serde_json::json!(null));
    tool_deps.insert("Approval".into(), serde_json::json!(null));
    tool_deps.insert("LaborMarket".into(), serde_json::json!(null));
    tool_deps.insert("Environment".into(), serde_json::json!(null));

    let mut tools = agent_spec.allowed_tools.unwrap_or_else(|| agent_spec.tools.clone());
    if !agent_spec.exclude_tools.is_empty() {
        tracing::debug!("Excluding tools: {:?}", agent_spec.exclude_tools);
        tools.retain(|tool| !agent_spec.exclude_tools.contains(tool));
    }
    toolset.load_tools(&tools, tool_deps).await?;

    // Add the Agent tool if requested and not excluded.
    if tools.contains(&"Agent".to_string()) && !agent_spec.exclude_tools.contains(&"Agent".to_string()) {
        toolset.add(Arc::new(crate::tools::agent::Agent::new(runtime.clone()))).await;
    }

    let mut plugin_manager = crate::plugin::manager::PluginManager::default();
    let plugin_tools = plugin_manager.load(&runtime.config, &runtime.approval);
    for result in plugin_manager.load_results() {
        if let Some(ref err) = result.error {
            tracing::warn!(
                plugin = %result.plugin_name,
                path = %result.manifest_path.display(),
                "failed to load plugin: {}",
                err
            );
        }
    }
    for plugin_tool in plugin_tools {
        if toolset.find(&plugin_tool.name()).await.is_some() {
            tracing::warn!(
                "Plugin tool '{}' conflicts with an existing tool, skipping",
                plugin_tool.name()
            );
            continue;
        }
        toolset.add(plugin_tool).await;
    }

    // Post-init bindings for wire hub, approval runtime, notifications, environment, and hooks.
    if let Some(ref hub) = runtime.root_wire_hub {
        runtime.approval.runtime().bind_root_wire_hub(hub);
        runtime.notifications.bind_root_wire_hub(hub);
        runtime.environment.bind_root_wire_hub(hub);
    }
    for hook in &runtime.config.hooks {
        if let Some(engine) = toolset.hook_engine_mut() {
            engine.add_hook(hook.clone());
        }
    }

    if !mcp_configs.is_empty() {
        if start_mcp_loading {
            toolset.load_mcp_tools(mcp_configs, runtime, true).await?;
        } else {
            toolset.defer_mcp_tool_loading(mcp_configs, runtime).await;
        }
    }

    Ok(Agent {
        name: agent_spec.name,
        system_prompt,
        toolset,
        runtime: runtime.clone(),
    })
}

fn _load_system_prompt(
    path: &Path,
    args: &HashMap<String, String>,
    builtin_args: &BuiltinSystemPromptArgs,
) -> crate::error::Result<String> {
    tracing::info!("Loading system prompt: {}", path.display());
    let system_prompt = std::fs::read_to_string(path)?
        .trim()
        .to_string();
    tracing::debug!(
        "Substituting system prompt with builtin args: {:?}, spec args: {:?}",
        builtin_args,
        args
    );

    // Preprocess ${var} syntax to {{ var }} for minijinja compatibility.
    let re = regex::Regex::new(r"\$\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}")
        .map_err(|e| crate::error::KimiCliError::Generic(format!("Regex error: {e}")))?;
    let converted = re.replace_all(&system_prompt, "{{ $1 }}");

    let env = minijinja::Environment::new();

    let mut combined: HashMap<String, String> = HashMap::new();
    combined.insert("KIMI_NOW".into(), builtin_args.KIMI_NOW.clone());
    combined.insert("KIMI_WORK_DIR".into(), builtin_args.KIMI_WORK_DIR.to_string_lossy().to_string());
    combined.insert("KIMI_WORK_DIR_LS".into(), builtin_args.KIMI_WORK_DIR_LS.clone());
    combined.insert("KIMI_AGENTS_MD".into(), builtin_args.KIMI_AGENTS_MD.clone());
    combined.insert("KIMI_SKILLS".into(), builtin_args.KIMI_SKILLS.clone());
    combined.insert("KIMI_ADDITIONAL_DIRS_INFO".into(), builtin_args.KIMI_ADDITIONAL_DIRS_INFO.clone());
    combined.insert("KIMI_OS".into(), builtin_args.KIMI_OS.clone());
    combined.insert("KIMI_SHELL".into(), builtin_args.KIMI_SHELL.clone());
    for (k, v) in args {
        combined.insert(k.clone(), v.clone());
    }

    let template = env.template_from_str(&converted)
        .map_err(|e| crate::error::KimiCliError::SystemPromptTemplate(format!("Invalid system prompt template: {}: {}", path.display(), e)))?;
    let rendered = template.render(combined)
        .map_err(|e| crate::error::KimiCliError::SystemPromptTemplate(format!("Missing system prompt arg in {}: {}", path.display(), e)))?;

    Ok(rendered)
}
