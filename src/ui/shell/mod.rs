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
    ToolCall(String),
    ToolResult(String, String),
    // AssistantText(String), // currently unused
    Think(String),
    Notification(String),
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
    assistant_buffer: String,
}

impl TurnState {
    fn new() -> Self {
        Self {
            live_events: Vec::new(),
            modal: None,
            assistant_buffer: String::new(),
        }
    }
}

/// Interactive shell UI using ratatui.
#[derive(Debug, Clone, Default)]
pub struct ShellUi {
    history: Vec<HistoryItem>,
    input: String,
    scroll_offset: u16,
    status: Option<crate::soul::StatusSnapshot>,
    plan_mode: bool,
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
                            if !state.assistant_buffer.is_empty() {
                                let text = state.assistant_buffer.trim().to_string();
                                if !text.is_empty() && !text.eq("(no response)") {
                                    self.history.push(HistoryItem { role: "assistant", content: text });
                                }
                                state.assistant_buffer.clear();
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
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return None;
                }

                if let Some(outcome) = self.handle_shell_slash_command(&text) {
                    self.input.clear();
                    self.scroll_offset = 0;
                    return Some(outcome);
                }

                self.history.push(HistoryItem {
                    role: "user",
                    content: text.clone(),
                });
                self.input.clear();
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
            KeyCode::Char(c) => {
                self.input.push(c);
                None
            }
            KeyCode::Backspace => {
                self.input.pop();
                None
            }
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                None
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
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
            crate::wire::types::WireMessage::ToolCall { name, .. } => {
                state.live_events.push(LiveEvent::ToolCall(name));
            }
            crate::wire::types::WireMessage::ToolResult { result, .. } => {
                state.live_events.push(LiveEvent::ToolResult(
                    String::new(),
                    result.extract_text(),
                ));
            }
            crate::wire::types::WireMessage::TextPart { text } => {
                state.assistant_buffer.push_str(&text);
            }
            crate::wire::types::WireMessage::ThinkPart { thought } => {
                state.live_events.push(LiveEvent::Think(thought));
            }
            crate::wire::types::WireMessage::Notification { text } => {
                state.live_events.push(LiveEvent::Notification(text));
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

    fn draw(
        &self,
        frame: &mut ratatui::Frame,
        turn_state: &Arc<tokio::sync::Mutex<TurnState>>,
        running: &Arc<tokio::sync::Mutex<bool>>,
    ) {
        let state = turn_state.blocking_lock();
        let is_running = *running.blocking_lock();
        let has_modal = state.modal.is_some();

        let live_height = if state.live_events.is_empty() { 0 } else {
            (state.live_events.len() as u16 + 2).min(6)
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
        if !state.live_events.is_empty() {
            let live_text: Vec<Line> = state
                .live_events
                .iter()
                .map(|evt| match evt {
                    LiveEvent::StepBegin(n) => Line::from(vec![
                        Span::styled("Step ", Style::default().fg(Color::Yellow)),
                        Span::styled(n.to_string(), Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
                    ]),
                    LiveEvent::ToolCall(name) => Line::from(vec![
                        Span::styled("Tool: ", Style::default().fg(Color::Cyan)),
                        Span::raw(name),
                    ]),
                    LiveEvent::ToolResult(name, text) => Line::from(vec![
                        Span::styled("Result ", Style::default().fg(Color::DarkGray)),
                        Span::raw(format!("{name}: {text}")),
                    ]),
                    // LiveEvent::AssistantText(t) => Line::from(t.as_str()),
                    LiveEvent::Think(t) => Line::from(vec![
                        Span::styled("Think: ", Style::default().fg(Color::Magenta)),
                        Span::raw(t),
                    ]),
                    LiveEvent::Notification(t) => Line::from(vec![
                        Span::styled("Notify: ", Style::default().fg(Color::Yellow)),
                        Span::raw(t),
                    ]),
                    LiveEvent::McpLoading => Line::from(vec![
                        Span::styled("MCP ", Style::default().fg(Color::Cyan)),
                        Span::raw("loading..."),
                    ]),
                    LiveEvent::McpDone => Line::from(vec![
                        Span::styled("MCP ", Style::default().fg(Color::Green)),
                        Span::raw("ready"),
                    ]),
                })
                .collect();
            let live_paragraph = Paragraph::new(Text::from(live_text))
                .block(Block::default().title("Live").borders(Borders::ALL))
                .wrap(Wrap { trim: true });
            frame.render_widget(live_paragraph, chunks[1]);
        }

        // Status bar
        let status_idx = if state.live_events.is_empty() { 1 } else { 2 };
        let status_text = self.build_status_text(&state);
        let status_paragraph = Paragraph::new(status_text)
            .block(Block::default().title("Status").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(status_paragraph, chunks[status_idx]);

        // Input
        let input_idx = if state.live_events.is_empty() { 2 } else { 3 };
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
        let cursor_x = (input_area.x + 2 + self.input.len() as u16).min(input_area.x + input_area.width - 1);
        let cursor_y = input_area.y + 1;
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));

        // Modal overlay
        if has_modal {
            let area = Self::centered_rect(60, 40, frame.area());
            frame.render_widget(Clear, area);
            self.draw_modal(frame, area, &state);
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
        if !state.assistant_buffer.is_empty() {
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
}
