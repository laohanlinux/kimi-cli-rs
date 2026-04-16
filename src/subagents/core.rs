use std::path::Path;

/// Specification for a subagent run.
#[derive(Debug, Clone)]
pub struct SubagentRunSpec {
    pub agent_id: String,
    pub type_def: crate::subagents::labor_market::AgentTypeDefinition,
    pub launch_spec: crate::subagents::models::AgentLaunchSpec,
    pub prompt: String,
    pub resumed: bool,
}

/// Prepares a subagent soul for execution.
#[tracing::instrument(level = "debug", skip_all)]
pub async fn prepare_soul(
    spec: &SubagentRunSpec,
    runtime: &crate::soul::agent::Runtime,
    builder: &crate::subagents::builder::SubagentBuilder,
    store: &crate::subagents::store::SubagentStore,
) -> crate::error::Result<(crate::soul::kimisoul::KimiSoul, String)> {
    let agent = builder
        .build_builtin_instance(&spec.agent_id, &spec.type_def, &spec.launch_spec)
        .await?;

    let mut context = crate::soul::context::Context::new(store.context_path(&spec.agent_id));
    context.restore().await?;

    let agent = if !context.system_prompt().unwrap_or_default().is_empty() {
        crate::soul::agent::Agent {
            system_prompt: context.system_prompt().unwrap_or_default().to_string(),
            ..agent
        }
    } else {
        if !agent.system_prompt.is_empty() {
            context.write_system_prompt(&agent.system_prompt).await?;
        }
        agent
    };

    let mut prompt = spec.prompt.clone();
    if spec.type_def.name == "explore" && !spec.resumed {
        if let Some(git_ctx) = collect_git_context(&runtime.builtin_args.KIMI_WORK_DIR).await {
            prompt = format!("{git_ctx}\n\n{prompt}");
        }
    }

    std::fs::write(store.prompt_path(&spec.agent_id), &prompt).ok();

    let soul = crate::soul::kimisoul::KimiSoul::new(agent, context, runtime.clone());
    Ok((soul, prompt))
}

async fn collect_git_context(work_dir: &Path) -> Option<String> {
    let git_dir = work_dir.join(".git");
    if !git_dir.exists() {
        return None;
    }

    let output = tokio::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(work_dir)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let status = String::from_utf8_lossy(&output.stdout);
    if status.trim().is_empty() {
        return None;
    }

    Some(format!(
        "<git-context>\n```\n{}\n```\n</git-context>",
        status.trim()
    ))
}
