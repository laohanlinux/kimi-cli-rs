use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Terminal;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

mod visualize;

/// A single turn in the shell history.
#[derive(Debug, Clone)]
struct HistoryItem {
    role: &'static str,
    content: String,
}

/// Live event displayed during a turn.
#[derive(Debug, Clone)]
enum LiveEvent {
    StepBegin(usize),
    ToolCall(visualize::ToolCallBlock),
    Think(String),
    Notification(visualize::NotificationBlock),
    McpLoading,
    McpDone,
}

/// Modal overlay state.
#[derive(Debug, Clone)]
enum Modal {
    Approval {
        request_id: String,
        _tool_call_id: String,
        sender: String,
        action: String,
        description: String,
        selected_index: usize,
    },
    Question {
        request_id: String,
        items: Vec<crate::wire::types::QuestionItem>,
        current_item: usize,
        selected_index: usize,
        multi_selected: std::collections::HashSet<usize>,
        answers: HashMap<String, String>,
    },
}

/// Runtime state for a single turn.
#[derive(Debug, Clone)]
struct TurnState {
    live_events: Vec<LiveEvent>,
    modal: Option<Modal>,
    content_block: visualize::ContentBlock,
}

impl TurnState {
    fn new() -> Self {
        Self {
            live_events: Vec::new(),
            modal: None,
            content_block: visualize::ContentBlock::new(false),
        }
    }
}

/// Popup overlay state for the shell.
#[derive(Debug, Clone)]
enum Popup {
    Help,
    Replay(Vec<String>, usize),
    SessionPicker(Vec<(String, String)>, usize),
    TaskList(Vec<String>, usize),
}

/// Interactive shell UI using ratatui.
#[derive(Debug, Clone, Default)]
pub struct ShellUi {
    history: Vec<HistoryItem>,
    input: String,
    cursor: usize,
    input_history: Vec<String>,
    input_history_index: Option<usize>,
    scroll_offset: u16,
    status: Option<crate::soul::StatusSnapshot>,
    plan_mode: bool,
    popup: Option<Popup>,
}

impl ShellUi {
    pub async fn run(
        &mut self,
        cli: crate::app::KimiCLI,
    ) -> crate::error::Result<crate::app::ShellOutcome> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(
            &mut stdout,
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal, cli).await;

        let mut stdout = io::stdout();
        disable_raw_mode()?;
        crossterm::execute!(
            &mut stdout,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        cli: crate::app::KimiCLI,
    ) -> crate::error::Result<crate::app::ShellOutcome> {
        let (wire_tx, mut wire_rx) = tokio::sync::mpsc::channel::<crate::wire::types::WireMessage>(256);
        let cli_arc = Arc::new(tokio::sync::Mutex::new(Some(cli)));
        let turn_state = Arc::new(tokio::sync::Mutex::new(TurnState::new()));
        let running = Arc::new(tokio::sync::Mutex::new(false));
        let mut outcome_rx_opt: Option<tokio::sync::oneshot::Receiver<crate::error::Result<crate::soul::TurnOutcome>>> = None;

        loop {
            terminal.draw(|f| self.draw(f, &turn_state, &running))?;

            let event_fut = tokio::task::spawn_blocking(|| crossterm::event::read());

            tokio::select! {
                biased;
                Some(msg) = wire_rx.recv() => {
                    let mut state = turn_state.lock().await;
                    self.handle_wire_message(&mut state, msg, &wire_tx).await;
                }
                event_result = event_fut => {
                    match event_result {
                        Ok(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                            if let Some(outcome) = self.handle_key(
                                key,
                                &turn_state,
                                &running,
                                &cli_arc,
                                &wire_tx,
                                &mut outcome_rx_opt,
                            ).await {
                                if matches!(outcome, crate::app::ShellOutcome::Exit) {
                                    let mut state = turn_state.lock().await;
                                    if state.modal.is_some() {
                                        self.cancel_modal(&mut state, &wire_tx).await;
                                        continue;
                                    }
                                }
                                break Ok(outcome);
                            }
                        }
                        Ok(Ok(Event::Resize(_, _))) => {}
                        _ => {}
                    }
                }
                Some(ref mut outcome_rx) = async { outcome_rx_opt.as_mut() }, if outcome_rx_opt.is_some() => {
                    match outcome_rx.await {
                        Ok(outcome_result) => {
                            *running.lock().await = false;
                            outcome_rx_opt = None;
                            let mut state = turn_state.lock().await;
                            if !state.content_block.is_empty() {
                                let text = state.content_block.text().trim().to_string();
                                if !text.is_empty() && !text.eq("(no response)") {
                                    self.history.push(HistoryItem { role: "assistant", content: text });
                                }
                                state.content_block.clear();
                            }
                            state.live_events.clear();
                            state.modal = None;
                            drop(state);
                            match outcome_result {
                                Ok(outcome) => {
                                    if let Some(msg) = outcome.final_message {
                                        let text = msg.extract_text("");
                                        if !text.is_empty() {
                                            if self.history.last().map(|h| &h.content) != Some(&text) {
                                                self.history.push(HistoryItem { role: "assistant", content: text });
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Turn failed: {}", e);
                                    self.history.push(HistoryItem { role: "system", content: format!("Error: {e}") });
                                }
                            }
                        }
                        Err(_) => {
                            *running.lock().await = false;
                            outcome_rx_opt = None;
                        }
                    }
                }
            }
        }
    }

    async fn handle_key(
        &mut self,
        key: KeyEvent,
        turn_state: &Arc<tokio::sync::Mutex<TurnState>>,
        running: &Arc<tokio::sync::Mutex<bool>>,
        cli_arc: &Arc<tokio::sync::Mutex<Option<crate::app::KimiCLI>>>,
        wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>,
        outcome_rx_opt: &mut Option<tokio::sync::oneshot::Receiver<crate::error::Result<crate::soul::TurnOutcome>>>,
    ) -> Option<crate::app::ShellOutcome> {
        if self.popup.is_some() {
            return self.handle_popup_key(key, cli_arc).await;
        }

        {
            let mut state = turn_state.lock().await;
            if state.modal.is_some() {
                return self.handle_modal_key(&mut state, key, wire_tx).await;
            }
        }

        if *running.lock().await {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {}
                _ => {}
            }
            return None;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(crate::app::ShellOutcome::Exit);
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(crate::app::ShellOutcome::Exit);
            }
            KeyCode::Esc => return Some(crate::app::ShellOutcome::Exit),
            KeyCode::Enter => {
                let text = self.input.trim_end().to_string();
                if text.is_empty() {
                    return None;
                }

                if let Some(outcome) = self.handle_shell_slash_command(&text) {
                    self.input.clear();
                    self.cursor = 0;
                    self.input_history_index = None;
                    self.scroll_offset = 0;
                    return Some(outcome);
                }

                self.history.push(HistoryItem {
                    role: "user",
                    content: text.clone(),
                });
                if !self.input_history.contains(&text) {
                    self.input_history.push(text.clone());
                }
                self.input.clear();
                self.cursor = 0;
                self.input_history_index = None;
                self.scroll_offset = 0;

                {
                    let mut state = turn_state.lock().await;
                    *state = TurnState::new();
                }
                *running.lock().await = true;

                let parts = vec![crate::soul::message::ContentPart::Text { text }];
                let cli_arc = cli_arc.clone();
                let wire_tx = wire_tx.clone();
                let (outcome_tx, outcome_rx) = tokio::sync::oneshot::channel();
                *outcome_rx_opt = Some(outcome_rx);
                tokio::spawn(async move {
                    let mut cli = cli_arc.lock().await.take().expect("cli should be present");
                    let result = cli.run_with_wire(parts, {
                        let wire_tx = wire_tx.clone();
                        move |wire| {
                            Box::pin(async move {
                                let mut ui_side = wire.ui_side();
                                while let Some(msg) = ui_side.recv().await {
                                    if wire_tx.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                            })
                        }
                    }).await;
                    let _ = outcome_tx.send(result);
                    *cli_arc.lock().await = Some(cli);
                });
                None
            }
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match c {
                    'a' => {
                        self.cursor = 0;
                    }
                    'e' => {
                        self.cursor = self.input.len();
                    }
                    'u' => {
                        self.input.clear();
                        self.cursor = 0;
                    }
                    'k' => {
                        self.input.truncate(self.cursor);
                    }
                    'w' => {
                        let prev = self.input[..self.cursor]
                            .trim_end_matches(|c: char| c.is_ascii_whitespace())
                            .rfind(|c: char| c.is_ascii_whitespace())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        self.input.replace_range(prev..self.cursor, "");
                        self.cursor = prev;
                    }
                    'l' => {
                        self.scroll_offset = 0;
                    }
                    'h' => {
                        self.popup = Some(Popup::Help);
                    }
                    'r' => {
                        let events = self.gather_replay_events(cli_arc).await;
                        self.popup = Some(Popup::Replay(events, 0));
                    }
                    's' => {
                        let sessions = self.gather_sessions().await;
                        self.popup = Some(Popup::SessionPicker(sessions, 0));
                    }
                    't' => {
                        let tasks = self.gather_tasks(cli_arc).await;
                        self.popup = Some(Popup::TaskList(tasks, 0));
                    }
                    'c' => {}
                    _ => {}
                }
                None
            }
            KeyCode::Char(c) => {
                if self.cursor >= self.input.len() {
                    self.input.push(c);
                } else {
                    self.input.insert(self.cursor, c);
                }
                self.cursor += 1;
                self.input_history_index = None;
                None
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                if self.cursor < self.input.len() {
                    self.cursor += 1;
                }
                None
            }
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                None
            }
            KeyCode::Up => {
                if !self.input_history.is_empty() {
                    let idx = self.input_history_index.map(|i| i.saturating_sub(1)).unwrap_or(self.input_history.len() - 1);
                    if idx < self.input_history.len() {
                        self.input = self.input_history[idx].clone();
                        self.cursor = self.input.len();
                        self.input_history_index = Some(idx);
                    }
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_add(1);
                }
                None
            }
            KeyCode::Down => {
                if let Some(idx) = self.input_history_index {
                    if idx + 1 < self.input_history.len() {
                        self.input_history_index = Some(idx + 1);
                        self.input = self.input_history[idx + 1].clone();
                        self.cursor = self.input.len();
                    } else {
                        self.input.clear();
                        self.cursor = 0;
                        self.input_history_index = None;
                    }
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_shell_slash_command(
        &mut self,
        text: &str,
    ) -> Option<crate::app::ShellOutcome> {
        if let Some(cmd_text) = text.strip_prefix('/') {
            let parts: Vec<&str> = cmd_text.splitn(2, ' ').collect();
            match parts[0] {
                "reload" => {
                    let prefill = parts.get(1).map(|s| s.to_string());
                    return Some(crate::app::ShellOutcome::Reload {
                        session_id: None,
                        prefill_text: prefill,
                    });
                }
                "new" => {
                    return Some(crate::app::ShellOutcome::Reload {
                        session_id: None,
                        prefill_text: None,
                    });
                }
                _ => {}
            }
        }
        None
    }

    async fn handle_wire_message(
        &mut self,
        state: &mut TurnState,
        msg: crate::wire::types::WireMessage,
        _wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>,
    ) {
        match msg {
            crate::wire::types::WireMessage::StepBegin { step_no } => {
                state.live_events.push(LiveEvent::StepBegin(step_no));
            }
            crate::wire::types::WireMessage::ToolCall { name, arguments, .. } => {
                let argument = crate::tools::extract_key_argument(
                    &arguments.to_string(),
                    &name,
                );
                state.live_events.push(LiveEvent::ToolCall(
                    visualize::ToolCallBlock::new(name, argument),
                ));
            }
            crate::wire::types::WireMessage::ToolResult { result, .. } => {
                // Find the most recent unfinished ToolCall block and finish it.
                for evt in state.live_events.iter_mut().rev() {
                    if let LiveEvent::ToolCall(block) = evt {
                        if !block.finished {
                            block.finish(&result);
                            break;
                        }
                    }
                }
            }
            crate::wire::types::WireMessage::TextPart { text } => {
                state.content_block.append(&text);
            }
            crate::wire::types::WireMessage::ThinkPart { thought } => {
                state.live_events.push(LiveEvent::Think(thought));
            }
            crate::wire::types::WireMessage::Notification { text } => {
                state.live_events.push(LiveEvent::Notification(
                    visualize::NotificationBlock::new("Notification", text, "info"),
                ));
            }
            crate::wire::types::WireMessage::McpLoadingBegin => {
                state.live_events.push(LiveEvent::McpLoading);
            }
            crate::wire::types::WireMessage::McpLoadingEnd => {
                state.live_events.push(LiveEvent::McpDone);
            }
            crate::wire::types::WireMessage::StatusUpdate { snapshot } => {
                self.status = Some(crate::soul::StatusSnapshot {
                    context_usage: snapshot.context_usage,
                    yolo_enabled: snapshot.yolo_enabled,
                    plan_mode: snapshot.plan_mode,
                    context_tokens: snapshot.context_tokens,
                    max_context_tokens: snapshot.max_context_tokens,
                    mcp_status: snapshot.mcp_status,
                });
                self.plan_mode = snapshot.plan_mode;
            }
            crate::wire::types::WireMessage::ApprovalRequest {
                id,
                tool_call_id,
                sender,
                action,
                description,
                ..
            } => {
                state.modal = Some(Modal::Approval {
                    request_id: id,
                    _tool_call_id: tool_call_id,
                    sender,
                    action,
                    description,
                    selected_index: 0,
                });
            }
            crate::wire::types::WireMessage::QuestionRequest { id, items } => {
                state.modal = Some(Modal::Question {
                    request_id: id,
                    items,
                    current_item: 0,
                    selected_index: 0,
                    multi_selected: std::collections::HashSet::new(),
                    answers: HashMap::new(),
                });
            }
            _ => {}
        }
    }

    async fn handle_modal_key(
        &mut self,
        state: &mut TurnState,
        key: KeyEvent,
        wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>,
    ) -> Option<crate::app::ShellOutcome> {
        match &mut state.modal {
            Some(Modal::Approval { selected_index, .. }) => {
                match key.code {
                    KeyCode::Up => {
                        *selected_index = selected_index.saturating_sub(1);
                        None
                    }
                    KeyCode::Down => {
                        *selected_index = (*selected_index + 1).min(3);
                        None
                    }
                    KeyCode::Char('1') => { *selected_index = 0; None }
                    KeyCode::Char('2') => { *selected_index = 1; None }
                    KeyCode::Char('3') => { *selected_index = 2; None }
                    KeyCode::Char('4') => { *selected_index = 3; None }
                    KeyCode::Enter => {
                        self.submit_approval(state, wire_tx).await;
                        None
                    }
                    KeyCode::Esc => {
                        self.cancel_modal(state, wire_tx).await;
                        None
                    }
                    _ => None,
                }
            }
            Some(Modal::Question {
                items,
                current_item,
                selected_index,
                multi_selected,
                ..
            }) => {
                let q = &items[*current_item];
                let option_count = q.options.len() + 1;
                match key.code {
                    KeyCode::Up => {
                        *selected_index = selected_index.saturating_sub(1);
                        None
                    }
                    KeyCode::Down => {
                        *selected_index = (*selected_index + 1).min(option_count - 1);
                        None
                    }
                    KeyCode::Char(' ') if q.multi_select => {
                        if multi_selected.contains(selected_index) {
                            multi_selected.remove(selected_index);
                        } else {
                            multi_selected.insert(*selected_index);
                        }
                        None
                    }
                    KeyCode::Enter => {
                        self.submit_question(state, wire_tx).await;
                        None
                    }
                    KeyCode::Esc => {
                        self.cancel_modal(state, wire_tx).await;
                        None
                    }
                    _ => None,
                }
            }
            None => None,
        }
    }

    async fn submit_approval(&mut self, state: &mut TurnState, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Approval { request_id, selected_index, .. }) = state.modal.take() {
            let response_str = match selected_index {
                0 => "approve",
                1 => "approve_for_session",
                _ => "reject",
            }.to_string();
            let _ = wire_tx.send(crate::wire::types::WireMessage::ApprovalResponse {
                request_id,
                response: response_str,
                feedback: None,
            }).await;
        }
    }

    async fn submit_question(&mut self, state: &mut TurnState, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Question { request_id, answers, .. }) = state.modal.take() {
            let _ = wire_tx.send(crate::wire::types::WireMessage::QuestionResponse {
                request_id,
                answers: answers.clone(),
            }).await;
        }
    }

    async fn cancel_modal(&mut self, state: &mut TurnState, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Approval { request_id, .. }) = state.modal.take() {
            let _ = wire_tx.send(crate::wire::types::WireMessage::ApprovalResponse {
                request_id,
                response: "reject".into(),
                feedback: None,
            }).await;
        } else if let Some(Modal::Question { request_id, .. }) = state.modal.take() {
            let _ = wire_tx.send(crate::wire::types::WireMessage::QuestionResponse {
                request_id: request_id.clone(),
                answers: HashMap::new(),
            }).await;
        }
    }

    async fn handle_popup_key(
        &mut self,
        key: KeyEvent,
        _cli_arc: &Arc<tokio::sync::Mutex<Option<crate::app::KimiCLI>>>,
    ) -> Option<crate::app::ShellOutcome> {
        match &mut self.popup {
            Some(Popup::Help) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') => {
                        self.popup = None;
                    }
                    _ => {}
                }
                None
            }
            Some(Popup::Replay(events, selected)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('r') => {
                        self.popup = None;
                    }
                    KeyCode::Up => {
                        *selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if *selected + 1 < events.len() {
                            *selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        // Replay selection: append to history as a system note
                        if let Some(evt) = events.get(*selected) {
                            self.history.push(HistoryItem {
                                role: "system",
                                content: format!("Replay: {evt}"),
                            });
                        }
                        self.popup = None;
                    }
                    _ => {}
                }
                None
            }
            Some(Popup::SessionPicker(sessions, selected)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => {
                        self.popup = None;
                    }
                    KeyCode::Up => {
                        *selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if *selected + 1 < sessions.len() {
                            *selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some((session_id, _)) = sessions.get(*selected) {
                            if session_id != "__empty__" {
                                return Some(crate::app::ShellOutcome::Reload {
                                    session_id: Some(session_id.clone()),
                                    prefill_text: None,
                                });
                            }
                        }
                        self.popup = None;
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        // Toggle scope would require state; for now just close
                        self.popup = None;
                    }
                    _ => {}
                }
                None
            }
            Some(Popup::TaskList(tasks, selected)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => {
                        self.popup = None;
                    }
                    KeyCode::Up => {
                        *selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if *selected + 1 < tasks.len() {
                            *selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(task) = tasks.get(*selected) {
                            self.history.push(HistoryItem {
                                role: "system",
                                content: format!("Task selected: {task}"),
                            });
                        }
                        self.popup = None;
                    }
                    KeyCode::Char('r') => {
                        // Refresh tasks
                        self.popup = None;
                    }
                    KeyCode::Char('s') => {
                        // Stop selected task would require CLI access
                        self.popup = None;
                    }
                    _ => {}
                }
                None
            }
            None => None,
        }
    }

    async fn gather_replay_events(
        &self,
        cli_arc: &Arc<tokio::sync::Mutex<Option<crate::app::KimiCLI>>>,
    ) -> Vec<String> {
        let cli_guard = cli_arc.lock().await;
        let Some(cli) = cli_guard.as_ref() else {
            return vec!["No CLI available.".into()];
        };
        let records = cli.soul().wire_file().records();
        let mut events = Vec::new();
        for record in records.into_iter().rev().take(20) {
            let text = match record {
                crate::wire::types::WireMessage::TextPart { text } => format!("Text: {}", text.chars().take(60).collect::<String>()),
                crate::wire::types::WireMessage::ThinkPart { thought } => format!("Think: {}", thought.chars().take(60).collect::<String>()),
                crate::wire::types::WireMessage::ToolCall { name, .. } => format!("ToolCall: {name}"),
                crate::wire::types::WireMessage::ToolResult { result, .. } => format!("ToolResult: {}", result.extract_text().chars().take(60).collect::<String>()),
                crate::wire::types::WireMessage::StepBegin { step_no } => format!("Step {step_no}"),
                crate::wire::types::WireMessage::TurnBegin { .. } => "Turn begin".into(),
                crate::wire::types::WireMessage::Notification { text } => format!("Notify: {text}"),
                _ => format!("{:?}", record),
            };
            events.push(text);
        }
        if events.is_empty() {
            events.push("No replay events available.".into());
        }
        events
    }

    async fn gather_sessions(&self) -> Vec<(String, String)> {
        let sessions = crate::session::list_all().await;
        if sessions.is_empty() {
            return vec![("__empty__".into(), "No sessions found.".into())];
        }
        sessions
            .into_iter()
            .map(|s| {
                let label = format!("{} · {}", s.title, &s.id[..s.id.len().min(8)]);
                (s.id, label)
            })
            .collect()
    }

    async fn gather_tasks(
        &self,
        cli_arc: &Arc<tokio::sync::Mutex<Option<crate::app::KimiCLI>>>,
    ) -> Vec<String> {
        let cli_guard = cli_arc.lock().await;
        let Some(cli) = cli_guard.as_ref() else {
            return vec!["No CLI available.".into()];
        };
        let tasks = cli.soul().runtime.background_tasks.list(false).await;
        if tasks.is_empty() {
            return vec!["No background tasks.".into()];
        }
        tasks
            .into_iter()
            .map(|t| {
                let status = if t.is_running_blocking() { "running" } else { "done" };
                format!("[{}] {} · {}", status, t.command, t.id)
            })
            .collect()
    }

    fn draw(
        &self,
        frame: &mut ratatui::Frame,
        turn_state: &Arc<tokio::sync::Mutex<TurnState>>,
        running: &Arc<tokio::sync::Mutex<bool>>,
    ) {
        let state = turn_state.blocking_lock();
        let is_running = *running.blocking_lock();
        let has_modal = state.modal.is_some();

        let has_live = !state.live_events.is_empty() || !state.content_block.is_empty();
        let live_height = if has_live {
            let base = state.live_events.len() as u16;
            let content_lines = if state.content_block.is_empty() { 0 } else { 3u16 };
            (base + content_lines + 2).min(8)
        } else {
            0
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(live_height),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(frame.area());

        // History
        let history_text = self
            .history
            .iter()
            .flat_map(|item| {
                let color = match item.role {
                    "user" => Color::Cyan,
                    "assistant" => Color::Green,
                    "system" => Color::Red,
                    _ => Color::Gray,
                };
                vec![
                    Line::from(vec![Span::styled(
                        format!("[{}]", item.role),
                        Style::default().fg(color).add_modifier(ratatui::style::Modifier::BOLD),
                    )]),
                    Line::from(item.content.as_str()),
                    Line::from(""),
                ]
            })
            .collect::<Vec<_>>();

        let history_paragraph = Paragraph::new(Text::from(history_text))
            .block(Block::default().title("History").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((self.scroll_offset, 0));
        frame.render_widget(history_paragraph, chunks[0]);

        // Live events
        let has_live_content = !state.live_events.is_empty() || !state.content_block.is_empty();
        if has_live_content {
            let mut live_text: Vec<Line> = Vec::new();
            if !state.content_block.is_empty() {
                live_text.extend(state.content_block.render());
                live_text.push(Line::from(""));
            }
            for evt in &state.live_events {
                match evt {
                    LiveEvent::StepBegin(n) => live_text.push(Line::from(vec![
                        Span::styled("Step ", Style::default().fg(Color::Yellow)),
                        Span::styled(n.to_string(), Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
                    ])),
                    LiveEvent::ToolCall(block) => {
                        live_text.extend(block.render());
                    }
                    LiveEvent::Think(t) => live_text.push(Line::from(vec![
                        Span::styled("Think: ", Style::default().fg(Color::Magenta)),
                        Span::raw(t),
                    ])),
                    LiveEvent::Notification(block) => {
                        live_text.extend(block.render());
                    }
                    LiveEvent::McpLoading => live_text.push(Line::from(vec![
                        Span::styled("MCP ", Style::default().fg(Color::Cyan)),
                        Span::raw("loading..."),
                    ])),
                    LiveEvent::McpDone => live_text.push(Line::from(vec![
                        Span::styled("MCP ", Style::default().fg(Color::Green)),
                        Span::raw("ready"),
                    ])),
                }
            }
            let live_paragraph = Paragraph::new(Text::from(live_text))
                .block(Block::default().title("Live").borders(Borders::ALL))
                .wrap(Wrap { trim: true });
            frame.render_widget(live_paragraph, chunks[1]);
        }

        // Status bar
        let status_idx = if has_live { 2 } else { 1 };
        let status_text = self.build_status_text(&state);
        let status_paragraph = Paragraph::new(status_text)
            .block(Block::default().title("Status").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(status_paragraph, chunks[status_idx]);

        // Input
        let input_idx = if has_live { 3 } else { 2 };
        let input_text = format!("> {}", self.input);
        let mut input_title = if self.plan_mode {
            "Input [PLAN MODE] (Enter=send, Ctrl+C/Esc=quit)"
        } else {
            "Input (Enter=send, Ctrl+C/Esc=quit)"
        };
        if is_running {
            input_title = "Running... (Ctrl+C to cancel)";
        }
        let input_paragraph = Paragraph::new(input_text)
            .block(Block::default().title(input_title).borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(input_paragraph, chunks[input_idx]);

        // Cursor
        let input_area = chunks[input_idx];
        let prefix_len = 2u16; // "> "
        let avail_width = input_area.width.saturating_sub(2);
        let cursor_pos = self.cursor as u16;
        let cursor_line = if avail_width == 0 {
            0
        } else {
            (prefix_len + cursor_pos) / avail_width
        };
        let cursor_col = (prefix_len + cursor_pos) % avail_width;
        let cursor_x = (input_area.x + 1 + cursor_col).min(input_area.x + input_area.width - 1);
        let cursor_y = (input_area.y + 1 + cursor_line).min(input_area.y + input_area.height - 1);
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));

        // Modal overlay
        if has_modal {
            let area = Self::centered_rect(60, 40, frame.area());
            frame.render_widget(Clear, area);
            self.draw_modal(frame, area, &state);
        }

        // Popup overlay
        if let Some(ref popup) = self.popup {
            let area = Self::centered_rect(70, 50, frame.area());
            frame.render_widget(Clear, area);
            self.draw_popup(frame, area, popup);
        }
    }

    fn draw_popup(&self, frame: &mut ratatui::Frame, area: Rect, popup: &Popup) {
        match popup {
            Popup::Help => {
                let lines = vec![
                    Line::from(vec![Span::styled("Shortcuts", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD))]),
                    Line::from(""),
                    Line::from("Ctrl+C / Esc / Ctrl+Q  Exit shell"),
                    Line::from("Ctrl+H                  Show this help"),
                    Line::from("Ctrl+R                  Replay recent events"),
                    Line::from("Ctrl+S                  Session picker"),
                    Line::from("Ctrl+T                  Task browser"),
                    Line::from("Ctrl+L                  Scroll to top"),
                    Line::from("Ctrl+A                  Move cursor to start"),
                    Line::from("Ctrl+E                  Move cursor to end"),
                    Line::from("Ctrl+U                  Clear input"),
                    Line::from("Ctrl+K                  Clear from cursor to end"),
                    Line::from("Ctrl+W                  Delete previous word"),
                    Line::from("Up/Down                 Input history / scroll"),
                    Line::from(""),
                    Line::from(vec![Span::styled("Press Esc or H to close", Style::default().fg(Color::DarkGray))]),
                ];
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title("Help").borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            Popup::Replay(events, selected) => {
                let title = format!("Replay ({} events)", events.len());
                let lines: Vec<Line> = events
                    .iter()
                    .enumerate()
                    .map(|(i, evt)| {
                        let style = if i == *selected {
                            Style::default().bg(Color::Blue).fg(Color::White)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(evt.as_str(), style))
                    })
                    .collect();
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            Popup::SessionPicker(sessions, selected) => {
                let title = format!("Sessions ({})", sessions.len());
                let lines: Vec<Line> = sessions
                    .iter()
                    .enumerate()
                    .map(|(i, (_id, label))| {
                        let style = if i == *selected {
                            Style::default().bg(Color::Blue).fg(Color::White)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(label.as_str(), style))
                    })
                    .collect();
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            Popup::TaskList(tasks, selected) => {
                let title = format!("Tasks ({})", tasks.len());
                let lines: Vec<Line> = tasks
                    .iter()
                    .enumerate()
                    .map(|(i, task)| {
                        let style = if i == *selected {
                            Style::default().bg(Color::Blue).fg(Color::White)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(task.as_str(), style))
                    })
                    .collect();
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
        }
    }

    fn build_status_text(&self, state: &TurnState) -> Text<'_> {
        let mut spans = vec![];
        if let Some(status) = &self.status {
            let status_str = crate::soul::format_context_status(
                status.context_usage,
                status.context_tokens,
                status.max_context_tokens,
            );
            spans.push(Span::raw(status_str));
            if status.yolo_enabled {
                spans.push(Span::raw(" | "));
                spans.push(Span::styled("YOLO", Style::default().fg(Color::Red)));
            }
            if let Some(mcp) = &status.mcp_status {
                spans.push(Span::raw(" | "));
                spans.push(Span::raw(format!(
                    "MCP {}/{} conn, {} tools",
                    mcp.connected, mcp.total, mcp.tools
                )));
            }
        } else {
            spans.push(Span::raw("Ready"));
        }
        if !state.content_block.is_empty() {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("Generating...", Style::default().fg(Color::Green)));
        }
        Text::from(Line::from(spans))
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    fn draw_modal(&self, frame: &mut ratatui::Frame, area: Rect, state: &TurnState) {
        match &state.modal {
            Some(Modal::Approval {
                sender,
                action,
                description,
                selected_index,
                ..
            }) => {
                let options = [
                    "Approve once",
                    "Approve for this session",
                    "Reject",
                    "Reject with feedback",
                ];
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("Approval request", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
                    ]),
                    Line::from(""),
                    Line::from(format!("{} wants to {}:", sender, action)),
                    Line::from(description.as_str()),
                    Line::from(""),
                ];
                for (i, opt) in options.iter().enumerate() {
                    let num = i + 1;
                    if i == *selected_index {
                        lines.push(Line::from(vec![
                            Span::styled("> ", Style::default().fg(Color::Cyan)),
                            Span::styled(format!("[{}] {}", num, opt), Style::default().fg(Color::Cyan)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(format!("[{}] {}", num, opt), Style::default().fg(Color::Gray)),
                        ]));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("1-4 choose, Enter confirm, Esc cancel", Style::default().fg(Color::DarkGray)),
                ]));
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title("Approval").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            Some(Modal::Question {
                items,
                current_item,
                selected_index,
                multi_selected,
                ..
            }) => {
                let q = &items[*current_item];
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled(format!("? {}", q.question), Style::default().fg(Color::Yellow)),
                    ]),
                ];
                if q.multi_select {
                    lines.push(Line::from("(SPACE to toggle, ENTER to submit)"));
                }
                lines.push(Line::from(""));
                let option_count = q.options.len() + 1;
                for (i, opt) in q.options.iter().enumerate() {
                    let prefix = if q.multi_select {
                        if multi_selected.contains(&i) { "[x] " } else { "[ ] " }
                    } else if i == *selected_index {
                        "> "
                    } else {
                        "  "
                    };
                    let style = if i == *selected_index {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{}{}", prefix, opt.label), style),
                        Span::styled(format!(" - {}", opt.description), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                let other_idx = option_count - 1;
                let prefix = if q.multi_select {
                    if multi_selected.contains(&other_idx) { "[x] " } else { "[ ] " }
                } else if other_idx == *selected_index {
                    "> "
                } else {
                    "  "
                };
                let style = if other_idx == *selected_index {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Gray)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{}Other", prefix), style),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Enter submit, Esc cancel", Style::default().fg(Color::DarkGray)),
                ]));
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(Block::default().title("Question").borders(Borders::ALL).border_style(Style::default().fg(Color::Gray)))
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_ui_default() {
        let ui = ShellUi::default();
        assert!(ui.history.is_empty());
        assert!(ui.input.is_empty());
    }

    #[test]
    fn shell_ui_history_push() {
        let mut ui = ShellUi::default();
        ui.history.push(HistoryItem {
            role: "user",
            content: "hello".into(),
        });
        assert_eq!(ui.history.len(), 1);
    }

    #[test]
    fn shell_ui_cursor_movement() {
        let mut ui = ShellUi::default();
        ui.input = "hello".into();
        ui.cursor = 5;
        // Simulate Left
        ui.cursor = ui.cursor.saturating_sub(1);
        assert_eq!(ui.cursor, 4);
        // Simulate Home
        ui.cursor = 0;
        assert_eq!(ui.cursor, 0);
        // Simulate End
        ui.cursor = ui.input.len();
        assert_eq!(ui.cursor, 5);
    }

    #[test]
    fn shell_ui_input_history_navigation() {
        let mut ui = ShellUi::default();
        ui.input_history = vec!["first".into(), "second".into()];
        // Up loads latest history
        let idx = ui.input_history_index.unwrap_or(ui.input_history.len() - 1);
        ui.input = ui.input_history[idx].clone();
        ui.cursor = ui.input.len();
        ui.input_history_index = Some(idx);
        assert_eq!(ui.input, "second");
        // Another up loads earlier
        let idx2 = ui.input_history_index.map(|i| i.saturating_sub(1)).unwrap_or(0);
        ui.input = ui.input_history[idx2].clone();
        ui.cursor = ui.input.len();
        ui.input_history_index = Some(idx2);
        assert_eq!(ui.input, "first");
    }
}
