/// Builds subagent instances from builtin type definitions.
#[derive(Debug, Clone)]
pub struct SubagentBuilder {
    root_runtime: crate::soul::agent::Runtime,
}

impl SubagentBuilder {
    pub fn new(runtime: crate::soul::agent::Runtime) -> Self {
        Self { root_runtime: runtime }
    }

    #[tracing::instrument(level = "debug", skip(self, type_def, launch_spec))]
    pub async fn build_builtin_instance(
        &self,
        agent_id: &str,
        type_def: &crate::subagents::labor_market::AgentTypeDefinition,
        launch_spec: &crate::subagents::models::AgentLaunchSpec,
    ) -> crate::error::Result<crate::soul::agent::Agent> {
        let effective_model = Self::resolve_effective_model(type_def, launch_spec);
        let llm_override = if let Some(ref alias) = effective_model {
            crate::llm::clone_llm_with_model_alias(
                self.root_runtime.llm.as_ref(),
                &self.root_runtime.config,
                Some(alias.as_str()),
            )
            .await?
        } else {
            self.root_runtime.llm.clone()
        };

        let runtime = self.root_runtime.copy_for_subagent(
            agent_id.to_string(),
            type_def.name.clone(),
            llm_override,
        );

        crate::soul::agent::load_agent(
            &type_def.agent_file,
            &runtime,
            vec![],
            false,
        )
        .await
    }

    fn resolve_effective_model(
        type_def: &crate::subagents::labor_market::AgentTypeDefinition,
        launch_spec: &crate::subagents::models::AgentLaunchSpec,
    ) -> Option<String> {
        launch_spec
            .model_override
            .clone()
            .or_else(|| launch_spec.effective_model.clone())
            .or_else(|| type_def.default_model.clone())
    }
}
