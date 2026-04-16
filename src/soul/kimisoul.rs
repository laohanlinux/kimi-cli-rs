use crate::soul::compaction::Compaction;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// The core agent execution engine.
pub struct KimiSoul {
    agent: crate::soul::agent::Agent,
    context: crate::soul::context::Context,
    pub runtime: crate::soul::agent::Runtime,
    pub plan_mode: bool,
    pub plan_session_id: Option<String>,
    steer_queue: mpsc::UnboundedReceiver<String>,
    steer_tx: mpsc::UnboundedSender<String>,
    approval_queue: mpsc::UnboundedReceiver<crate::wire::types::WireMessage>,
    approval_tx: mpsc::UnboundedSender<crate::wire::types::WireMessage>,
    pending_plan_activation_injection: bool,
    injection_providers: Vec<Box<dyn crate::soul::dynamic_injection::DynamicInjectionProvider>>,
    hook_engine: crate::hooks::engine::HookEngine,
    slash_commands: Vec<std::sync::Arc<crate::soul::slash::SlashCommand>>,
    slash_command_map: std::collections::HashMap<String, usize>,
}

impl KimiSoul {
    pub fn new(
        agent: crate::soul::agent::Agent,
        context: crate::soul::context::Context,
        runtime: crate::soul::agent::Runtime,
    ) -> Self {
        let (steer_tx, steer_queue) = mpsc::unbounded_channel();
        let (approval_tx, approval_queue) = mpsc::unbounded_channel();
        let plan_mode = runtime.session.state.plan_mode;
        let plan_session_id = runtime.session.state.plan_session_id.clone();
        let slash_commands = Self::build_slash_commands();
        let slash_command_map = Self::index_slash_commands(&slash_commands);

        Self {
            agent,
            context,
            runtime,
            plan_mode,
            plan_session_id,
            steer_queue,
            steer_tx,
            approval_queue,
            approval_tx,
            pending_plan_activation_injection: false,
            injection_providers: vec![
                Box::new(crate::soul::dynamic_injections::plan_mode::PlanModeInjectionProvider),
                Box::new(crate::soul::dynamic_injections::yolo_mode::YoloModeInjectionProvider),
            ],
            hook_engine: crate::hooks::engine::HookEngine::default(),
            slash_commands,
            slash_command_map,
        }
    }

    pub fn name(&self) -> &str {
        &self.agent.name
    }

    pub fn model_name(&self) -> String {
        self.runtime
            .llm
            .as_ref()
            .map(|l| l.model_name.clone())
            .unwrap_or_default()
    }

    pub fn thinking(&self) -> Option<bool> {
        self.runtime.llm.as_ref().and_then(|l| l.thinking)
    }

    pub fn model_capabilities(&self) -> std::collections::HashSet<crate::config::ModelCapability> {
        self.runtime.llm.as_ref()
            .map(|l| l.capabilities.clone())
            .unwrap_or_default()
    }

    pub fn status(&self) -> crate::soul::StatusSnapshot {
        let token_count = self.context.history().iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|p| match p {
                crate::soul::message::ContentPart::Text { text } => Some(text.len().div_ceil(4)),
                _ => None,
            })
            .sum();
        let max_size = self.runtime.llm.as_ref().map(|l| l.max_context_size).unwrap_or(128_000);
        let usage = if max_size > 0 {
            (token_count as f64 / max_size as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mcp_status = self.agent.toolset.mcp_status_snapshot();
        crate::soul::StatusSnapshot {
            context_usage: usage,
            yolo_enabled: self.runtime.approval.yolo_blocking(),
            plan_mode: self.plan_mode,
            context_tokens: token_count,
            max_context_tokens: max_size,
            mcp_status,
        }
    }

    pub fn approval_tx(&self) -> mpsc::UnboundedSender<crate::wire::types::WireMessage> {
        self.approval_tx.clone()
    }

    pub fn agent(&self) -> &crate::soul::agent::Agent {
        &self.agent
    }

    pub fn context(&self) -> &crate::soul::context::Context {
        &self.context
    }

    pub fn wire_file(&self) -> &crate::wire::file::WireFile {
        &self.runtime.session.wire_file
    }

    pub fn is_yolo(&self) -> bool {
        self.runtime.approval.yolo_blocking()
    }

    pub fn hook_engine(&self) -> &crate::hooks::engine::HookEngine {
        &self.hook_engine
    }

    pub fn set_hook_engine(&mut self, engine: crate::hooks::engine::HookEngine) {
        self.hook_engine = engine.clone();
        self.agent.toolset.set_hook_engine(engine);
    }

    pub fn add_injection_provider(&mut self, provider: Box<dyn crate::soul::dynamic_injection::DynamicInjectionProvider>) {
        self.injection_providers.push(provider);
    }

    async fn collect_injections(&self) -> Vec<crate::soul::dynamic_injection::DynamicInjection> {
        let mut injections = Vec::new();
        for provider in &self.injection_providers {
            let result = provider.get_injections(self.context.history(), self).await;
            injections.extend(result);
        }
        injections
    }

    fn ensure_plan_session_id(&mut self) {
        if self.plan_session_id.is_none() {
            let id = uuid::Uuid::new_v4().to_string().replace("-", "");
            self.plan_session_id = Some(id.clone());
            self.runtime.session.state.plan_session_id = Some(id);
            let _ = self.runtime.session.save_state();
        }
    }

    fn set_plan_mode_inner(&mut self, enabled: bool) -> bool {
        if enabled == self.plan_mode {
            return self.plan_mode;
        }
        self.plan_mode = enabled;
        if enabled {
            self.ensure_plan_session_id();
            self.pending_plan_activation_injection = true;
        } else {
            self.pending_plan_activation_injection = false;
            self.plan_session_id = None;
            self.runtime.session.state.plan_session_id = None;
            self.runtime.session.state.plan_slug = None;
        }
        self.runtime.session.state.plan_mode = self.plan_mode;
        let _ = self.runtime.session.save_state();
        self.plan_mode
    }

    pub fn get_plan_file_path(&self) -> Option<PathBuf> {
        self.plan_session_id.as_ref().map(|id| {
            crate::share::get_share_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("plans")
                .join(format!("{id}.md"))
        })
    }

    pub fn read_current_plan(&self) -> Option<String> {
        let path = self.get_plan_file_path()?;
        std::fs::read_to_string(&path).ok()
    }

    pub fn clear_current_plan(&self) {
        if let Some(path) = self.get_plan_file_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    pub async fn toggle_plan_mode(&mut self) -> bool {
        let new_state = !self.plan_mode;
        self.set_plan_mode_inner(new_state)
    }

    pub async fn toggle_plan_mode_from_manual(&mut self) -> bool {
        self.toggle_plan_mode().await
    }

    pub async fn set_plan_mode(&mut self, enabled: bool) {
        self.set_plan_mode_inner(enabled);
    }

    pub async fn set_plan_mode_from_manual(&mut self, enabled: bool) -> bool {
        self.set_plan_mode_inner(enabled)
    }

    pub fn schedule_plan_activation_reminder(&mut self) {
        if self.plan_mode {
            self.pending_plan_activation_injection = true;
        }
    }

    pub fn consume_pending_plan_activation_injection(&mut self) -> bool {
        if !self.plan_mode || !self.pending_plan_activation_injection {
            return false;
        }
        self.pending_plan_activation_injection = false;
        true
    }

    fn should_auto_compact(&self) -> bool {
        let ratio = self.runtime.config.loop_control.compaction_trigger_ratio;
        if ratio <= 0.0 {
            return false;
        }
        let max_size = self.runtime.llm.as_ref().map(|l| l.max_context_size).unwrap_or(128_000);
        let token_count = self.context.history().iter()
            .flat_map(|m| m.content.iter())
            .filter_map(|p| match p {
                crate::soul::message::ContentPart::Text { text } => Some(text.len().div_ceil(4)),
                _ => None,
            })
            .sum::<usize>();
        (token_count as f64 / max_size as f64) >= ratio
    }

    /// Compacts the conversation context.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn compact_context(&mut self, custom_instruction: &str) {
        let compaction = crate::soul::compaction::SimpleCompaction::default();
        let history = self.context.history().to_vec();
        match compaction.compact(&history, &crate::llm::Llm::default(), custom_instruction).await {
            Ok(result) => {
                tracing::info!(
                    "compacted {} messages into {}",
                    history.len(),
                    result.messages.len()
                );
            }
            Err(e) => {
                tracing::error!("compaction failed: {}", e);
            }
        }
    }

    /// Clears the conversation context and rewrites the system prompt.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn clear_context(&mut self) {
        self.context = crate::soul::context::Context::new(self.runtime.session.context_file.clone());
        if let Err(e) = tokio::fs::remove_file(&self.runtime.session.context_file).await {
            tracing::debug!("no context file to remove: {}", e);
        }
        let prompt = self.agent.system_prompt.clone();
        if !prompt.is_empty() {
            if let Err(e) = self.context.write_system_prompt(&prompt).await {
                tracing::warn!("failed to rewrite system prompt: {}", e);
            }
        }
        tracing::info!("context cleared");
    }

    /// Queues a steer message for injection into the current turn.
    pub fn steer(&self, content: &str) {
        let _ = self.steer_tx.send(content.to_string());
    }

    async fn consume_pending_steers(&mut self) -> bool {
        let mut consumed = false;
        while let Ok(content) = self.steer_queue.try_recv() {
            tracing::debug!("consuming steer: {}", content);
            let steer_msg = crate::soul::message::Message {
                role: "user".into(),
                content: vec![crate::soul::message::ContentPart::Text { text: content }],
                tool_calls: None,
                tool_call_id: None,
            };
            if let Err(e) = self.context.append_message(&steer_msg).await {
                tracing::warn!("failed to inject steer message: {}", e);
            }
            consumed = true;
        }
        consumed
    }

    fn publish_wire(&self, msg: crate::wire::types::WireMessage) {
        if let Some(ref hub) = self.runtime.root_wire_hub {
            hub.publish(msg);
        }
    }

    /// Runs a single user turn.
    #[tracing::instrument(level = "info", skip(self))]
    pub async fn run(&mut self, user_input: Vec<crate::soul::message::ContentPart>) -> crate::error::Result<crate::soul::TurnOutcome> {
        tracing::info!("starting soul run");

        let _ = self.consume_pending_steers().await;

        // Trigger UserPromptSubmit hook.
        let engine = self.hook_engine.clone();
        let _ = engine.trigger("UserPromptSubmit", "", serde_json::json!({})).await;

        let mcp_started = self.start_background_mcp_loading().await;

        let text = user_input.iter().filter_map(|p| match p {
            crate::soul::message::ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        }).collect::<Vec<_>>().join("");

        // Handle slash commands.
        if let Some(cmd_text) = text.strip_prefix('/') {
            let parts: Vec<&str> = cmd_text.splitn(2, ' ').collect();
            let cmd_name = parts[0];
            let cmd_args = parts.get(1).unwrap_or(&"");
            if let Some(&idx) = self.slash_command_map.get(cmd_name) {
                if let Some(cmd) = self.slash_commands.get(idx).cloned() {
                    (cmd.handler)(self, cmd_args).await;
                    let reply = crate::soul::message::Message {
                        role: "assistant".into(),
                        content: vec![crate::soul::message::ContentPart::Text {
                            text: format!("Executed /{cmd_name}"),
                        }],
                        tool_calls: None,
                        tool_call_id: None,
                    };
                    let _ = self.context.append_message(&reply).await;
                    return Ok(crate::soul::TurnOutcome {
                        stop_reason: crate::soul::TurnStopReason::NoToolCalls,
                        final_message: Some(reply),
                        step_count: 0,
                    });
                }
            }
        }

        let user_message = crate::soul::message::Message {
            role: "user".into(),
            content: user_input,
            tool_calls: None,
            tool_call_id: None,
        };

        let stop_reason = self.turn(user_message).await?;

        if mcp_started {
            self.wait_for_background_mcp_loading().await;
        }

        // Retrieve the last assistant message as the final message, if any.
        let final_message = self.context.history().iter().rev().find(|m| m.role == "assistant").cloned();

        // Auto title generation on first turn.
        if self.runtime.session.state.title_generate_attempts == 0 {
            if let Some(ref msg) = final_message {
                let title_text = msg.extract_text("").chars().take(40).collect::<String>();
                if !title_text.is_empty() {
                    self.runtime.session.title = title_text;
                    let _ = self.runtime.session.save_state();
                }
            }
            self.runtime.session.state.title_generate_attempts += 1;
            let _ = self.runtime.session.save_state();
        }

        // Trigger Stop hook.
        let engine = self.hook_engine.clone();
        let _ = engine.trigger("Stop", "", serde_json::json!({})).await;

        Ok(crate::soul::TurnOutcome {
            stop_reason,
            final_message,
            step_count: 1,
        })
    }

    async fn turn(&mut self, user_message: crate::soul::message::Message) -> crate::error::Result<crate::soul::TurnStopReason> {
        let user_text = user_message.extract_text("");
        self.publish_wire(crate::wire::types::WireMessage::TurnBegin {
            user_input: user_text.clone(),
        });

        self.context.append_message(&user_message).await?;
        self.context.checkpoint(None).await?;
        self.runtime
            .denwa_renji
            .set_n_checkpoints(self.context.next_checkpoint_id());

        let stop_reason = self.agent_loop().await?;

        self.publish_wire(crate::wire::types::WireMessage::TurnEnd {
            stop_reason: format!("{:?}", stop_reason),
        });

        Ok(stop_reason)
    }

    /// Internal step loop.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn agent_loop(&mut self) -> crate::error::Result<crate::soul::TurnStopReason> {
        let max_steps = self.runtime.config.loop_control.max_steps_per_turn.max(1);
        for step_no in 0..max_steps {
            // Auto-compaction check.
            if self.should_auto_compact() {
                self.publish_wire(crate::wire::types::WireMessage::CompactionBegin);
                self.compact_context("").await;
                self.publish_wire(crate::wire::types::WireMessage::CompactionEnd);
            }

            match self.step(step_no).await? {
                Some(reason) => return Ok(reason),
                None => {
                    if let Some(dmail) = self.runtime.denwa_renji.fetch_pending_dmail() {
                        tracing::info!(
                            checkpoint_id = dmail.checkpoint_id,
                            "processing D-Mail"
                        );
                        self.context
                            .revert_to_checkpoint(dmail.checkpoint_id)
                            .await?;
                        self.publish_wire(crate::wire::types::WireMessage::Notification {
                            text: format!(
                                "Reverted to checkpoint {}: {}",
                                dmail.checkpoint_id, dmail.message
                            ),
                        });
                        return Ok(crate::soul::TurnStopReason::ToolRejected);
                    }
                    continue;
                }
            }
        }
        tracing::warn!("agent_loop reached max_steps ({})", max_steps);
        Ok(crate::soul::TurnStopReason::MaxStepsReached)
    }

    /// Executes one LLM step.
    #[tracing::instrument(level = "debug", skip(self))]
    async fn step(&mut self, step_no: usize) -> crate::error::Result<Option<crate::soul::TurnStopReason>> {
        self.publish_wire(crate::wire::types::WireMessage::StepBegin { step_no });
        let _ = self.consume_pending_steers().await;

        // Apply dynamic injections.
        let injections = self.collect_injections().await;
        for injection in injections {
            let msg = crate::soul::message::Message {
                role: "user".into(),
                content: vec![crate::soul::message::ContentPart::Text { text: injection.content }],
                tool_calls: None,
                tool_call_id: None,
            };
            self.context.append_message(&msg).await?;
        }

        while let Some(notification) = self.runtime.notifications.try_recv() {
            self.publish_wire(crate::wire::types::WireMessage::Notification {
                text: format!("{}: {}", notification.title, notification.body),
            });
        }

        let Some(ref llm) = self.runtime.llm else {
            let reply = crate::soul::message::Message {
                role: "assistant".into(),
                content: vec![crate::soul::message::ContentPart::Text {
                    text: "LLM client is not yet implemented in this Rust port.".into(),
                }],
                tool_calls: None,
                tool_call_id: None,
            };
            self.context.append_message(&reply).await?;
            return Ok(Some(crate::soul::TurnStopReason::NoToolCalls));
        };

        let system_prompt = self.agent.system_prompt.clone();
        let history = self.context.history().to_vec();
        let tools = &self.agent.toolset;

        let assistant_msg = llm
            .chat(Some(&system_prompt), &history, Some(tools))
            .await?;

        self.context.append_message(&assistant_msg).await?;

        // If the assistant requested tool calls, execute them and continue the loop.
        if let Some(ref tool_calls) = assistant_msg.tool_calls {
            if !tool_calls.is_empty() {
                let mut tool_results = Vec::new();
                for call in tool_calls {
                    // Approval check when not in YOLO mode.
                    let mut approved = self.runtime.approval.yolo_blocking();

                    if !approved {
                        // Evaluate approval runtime rules first.
                        if let Some(ref ar) = self.runtime.approval_runtime {
                            match ar.evaluate(&call.name, &call.arguments) {
                                crate::approval_runtime::ApprovalDecision::Approve => {
                                    tracing::info!(tool = %call.name, "auto-approved by approval runtime");
                                    approved = true;
                                }
                                crate::approval_runtime::ApprovalDecision::Deny { reason } => {
                                    tracing::info!(tool = %call.name, reason = %reason, "denied by approval runtime");
                                    let reject_result = crate::soul::message::ToolResult {
                                        tool_call_id: call.id.clone(),
                                        return_value: crate::soul::message::ToolReturnValue::Error {
                                            error: format!("Tool use was denied by approval runtime: {reason}"),
                                        },
                                    };
                                    tool_results.push(reject_result.clone());
                                    let result_msg = crate::soul::message::Message {
                                        role: "tool".into(),
                                        content: vec![crate::soul::message::ContentPart::Text {
                                            text: reject_result.return_value.extract_text(),
                                        }],
                                        tool_calls: None,
                                        tool_call_id: Some(reject_result.tool_call_id.clone()),
                                    };
                                    self.context.append_message(&result_msg).await?;
                                    continue;
                                }
                                crate::approval_runtime::ApprovalDecision::RequestUser => {
                                    // Fall through to wire approval request.
                                }
                            }
                        }
                    }

                    if !approved {
                        let req_id = uuid::Uuid::new_v4().to_string();
                        self.publish_wire(crate::wire::types::WireMessage::ApprovalRequest {
                            id: req_id.clone(),
                            tool_call_id: call.id.clone(),
                            sender: self.agent.name.clone(),
                            action: call.name.clone(),
                            description: format!("Call tool '{}' with args: {}", call.name, call.arguments),
                            display: None,
                        });
                        // Wait for approval response via the wire pump.
                        approved = match tokio::time::timeout(
                            tokio::time::Duration::from_secs(300),
                            self.approval_queue.recv(),
                        )
                        .await
                        {
                            Ok(Some(crate::wire::types::WireMessage::ApprovalResponse {
                                response,
                                ..
                            })) => response.to_lowercase() == "approve",
                            Ok(Some(_)) => false,
                            Ok(None) => false,
                            Err(_) => {
                                tracing::warn!("Approval request timed out for tool {}", call.name);
                                false
                            }
                        };
                        if !approved {
                            let reject_result = crate::soul::message::ToolResult {
                                tool_call_id: call.id.clone(),
                                return_value: crate::soul::message::ToolReturnValue::Error {
                                    error: "Tool use was rejected by user.".into(),
                                },
                            };
                            tool_results.push(reject_result.clone());
                            let result_msg = crate::soul::message::Message {
                                role: "tool".into(),
                                content: vec![crate::soul::message::ContentPart::Text {
                                    text: reject_result.return_value.extract_text(),
                                }],
                                tool_calls: None,
                                tool_call_id: Some(reject_result.tool_call_id.clone()),
                            };
                            self.context.append_message(&result_msg).await?;
                            continue;
                        }
                    }

                    let result = self.agent.toolset.handle(call, &self.runtime).await;
                    tool_results.push(result.clone());
                    let result_msg = crate::soul::message::tool_result_to_message(&result);
                    self.context.append_message(&result_msg).await?;

                    // Sync plan mode if a tool (e.g., EnterPlanMode/ExitPlanMode) modified persisted state.
                    let fresh_state = crate::session_state::load_session_state(&self.runtime.session.dir());
                    if fresh_state.plan_mode != self.plan_mode {
                        self.set_plan_mode_inner(fresh_state.plan_mode);
                    }
                }
                return Ok(None); // Continue stepping
            }
        }

        Ok(Some(crate::soul::TurnStopReason::NoToolCalls))
    }

    fn build_slash_commands() -> Vec<std::sync::Arc<crate::soul::slash::SlashCommand>> {
        crate::soul::slash::default_registry().into_commands()
    }

    fn index_slash_commands(commands: &[std::sync::Arc<crate::soul::slash::SlashCommand>]) -> std::collections::HashMap<String, usize> {
        commands
            .iter()
            .enumerate()
            .map(|(i, cmd)| (cmd.name.clone(), i))
            .collect()
    }

    pub fn available_slash_commands(&self) -> &[std::sync::Arc<crate::soul::slash::SlashCommand>] {
        &self.slash_commands
    }

    pub async fn start_background_mcp_loading(&mut self) -> bool {
        if self.agent.toolset.has_deferred_mcp_tools() {
            self.publish_wire(crate::wire::types::WireMessage::McpLoadingBegin);
            let started = self.agent.toolset.start_background_mcp_loading();
            if !started {
                self.publish_wire(crate::wire::types::WireMessage::McpLoadingEnd);
            }
            started
        } else {
            false
        }
    }

    pub async fn wait_for_background_mcp_loading(&mut self) {
        if self.agent.toolset.has_pending_mcp_tools()
            || self.agent.toolset.has_deferred_mcp_tools()
        {
            self.agent.toolset.wait_for_background_mcp_loading().await;
            self.publish_wire(crate::wire::types::WireMessage::McpLoadingEnd);
        }
    }
}
