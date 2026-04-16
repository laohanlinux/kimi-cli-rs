/// Request to run a foreground subagent.
#[derive(Debug, Clone)]
pub struct ForegroundRunRequest {
    pub description: String,
    pub prompt: String,
    pub requested_type: String,
    pub model: Option<String>,
    pub resume: Option<String>,
}

/// Runs a foreground subagent to completion.
#[derive(Debug, Clone)]
pub struct ForegroundSubagentRunner {
    runtime: crate::soul::agent::Runtime,
    store: crate::subagents::store::SubagentStore,
    builder: crate::subagents::builder::SubagentBuilder,
}

impl ForegroundSubagentRunner {
    pub fn new(runtime: crate::soul::agent::Runtime) -> Self {
        let store = runtime
            .subagent_store
            .clone()
            .unwrap_or_default();
        let builder = crate::subagents::builder::SubagentBuilder::new(runtime.clone());
        Self {
            runtime,
            store,
            builder,
        }
    }

    #[tracing::instrument(level = "info", skip(self, req))]
    pub async fn run(
        &self,
        req: &ForegroundRunRequest,
    ) -> crate::soul::message::ToolReturnValue {
        match self.run_inner(req).await {
            Ok(output) => crate::soul::message::ToolReturnValue::Ok {
                output,
                message: None,
            },
            Err((message, brief)) => crate::soul::message::ToolReturnValue::Error {
                error: format!("{brief}: {message}"),
            },
        }
    }

    async fn run_inner(
        &self,
        req: &ForegroundRunRequest,
    ) -> Result<String, (String, String)> {
        let (agent_id, actual_type, resumed) = self.prepare_instance(req).await?;

        let labor_market = self.runtime.labor_market.read().await;
        let type_def = labor_market
            .require_builtin_type(&actual_type)
            .map_err(|e| (e.to_string(), "Invalid subagent type".into()))?
            .clone();
        let default_model = type_def.default_model.clone();
        drop(labor_market);

        let launch_spec = crate::subagents::models::AgentLaunchSpec::new(
            agent_id.clone(),
            actual_type.clone(),
            req.model.clone(),
            req.model.clone().or_else(|| default_model),
        );

        let spec = crate::subagents::core::SubagentRunSpec {
            agent_id: agent_id.clone(),
            type_def: type_def.clone(),
            launch_spec,
            prompt: req.prompt.clone(),
            resumed,
        };

        let (mut soul, prompt) = crate::subagents::core::prepare_soul(
            &spec,
            &self.runtime,
            &self.builder,
            &self.store,
        )
        .await
        .map_err(|e| (e.to_string(), "Failed to prepare subagent".into()))?;

        self.store
            .update_instance(
                &agent_id,
                Some(crate::subagents::models::SubagentStatus::RunningForeground),
                None,
                None,
            )
            .ok();

        let parts = vec![crate::soul::message::ContentPart::Text { text: prompt }];
        let outcome = soul.run(parts).await;

        match outcome {
            Ok(result) => {
                let final_message = result
                    .final_message
                    .map(|m| m.extract_text(""))
                    .unwrap_or_default();
                self.store
                    .update_instance(
                        &agent_id,
                        Some(crate::subagents::models::SubagentStatus::Idle),
                        None,
                        None,
                    )
                    .ok();
                let mut lines = vec![
                    format!("agent_id: {agent_id}"),
                    format!("resumed: {resumed}"),
                    format!("actual_subagent_type: {actual_type}"),
                    "status: completed".into(),
                    "".into(),
                    "[summary]".into(),
                    final_message,
                ];
                if resumed && !req.requested_type.is_empty() && req.requested_type != actual_type {
                    lines.insert(2, format!("requested_subagent_type: {}", req.requested_type));
                }
                Ok(lines.join("\n"))
            }
            Err(e) => {
                self.store
                    .update_instance(
                        &agent_id,
                        Some(crate::subagents::models::SubagentStatus::Failed),
                        None,
                        None,
                    )
                    .ok();
                Err((e.to_string(), "Agent run failed".into()))
            }
        }
    }

    async fn prepare_instance(
        &self,
        req: &ForegroundRunRequest,
    ) -> Result<(String, String, bool), (String, String)> {
        if let Some(ref resume_id) = req.resume {
            let record = self
                .store
                .require_instance(resume_id)
                .map_err(|e| (e.to_string(), "Instance not found".into()))?;
            if matches!(
                record.status,
                crate::subagents::models::SubagentStatus::RunningForeground
                    | crate::subagents::models::SubagentStatus::RunningBackground
            ) {
                return Err((
                    format!(
                        "Agent instance {} is still {:?} and cannot be resumed concurrently.",
                        record.agent_id, record.status
                    ),
                    "Agent already running".into(),
                ));
            }
            return Ok((record.agent_id, record.subagent_type, true));
        }

        let actual_type = if req.requested_type.is_empty() {
            "coder".into()
        } else {
            req.requested_type.clone()
        };

        let agent_id = format!("a{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..8]);
        self.store.create_instance(
            &agent_id,
            &req.description,
            crate::subagents::models::AgentLaunchSpec::new(
                agent_id.clone(),
                actual_type.clone(),
                req.model.clone(),
                None,
            ),
        );
        Ok((agent_id, actual_type, false))
    }
}
