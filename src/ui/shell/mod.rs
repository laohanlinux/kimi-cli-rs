use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ui::repl::{
    self, draw, InputState, ReplMessage, ReplMessageRole,
};

/// Modal overlay state (Kimi approvals — not in Claude REPL, layered on top).
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

#[derive(Debug, Clone)]
enum Popup {
    Help,
    Replay(Vec<String>, usize),
    SessionPicker(Vec<(String, String)>, usize),
    TaskList(Vec<String>, usize),
}

/// Interactive shell: Claude Code REPL layout + Kimi Wire/Soul backend.
pub struct ShellUi {
    /// Ported from Claude `ReplApp::messages`.
    messages: Vec<ReplMessage>,
    /// Ported from Claude `ReplApp::input`.
    input: InputState,
    scroll_offset: usize,
    loading: bool,
    header_model: String,
    permission_label: String,
    plan_mode: bool,
    status: Option<crate::soul::StatusSnapshot>,
    popup: Option<Popup>,
    modal: Option<Modal>,
    session_dir: PathBuf,
    /// Session work directory (shown in footer).
    work_dir: PathBuf,
    exit: bool,
    running: bool,
    wire_tx: Option<tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>>,
    wire_rx: Option<tokio::sync::mpsc::Receiver<crate::wire::types::WireMessage>>,
    /// Bounded 1: one turn result per flight (`oneshot` + `now_or_never` drops the rx each poll → bug).
    outcome_rx: Option<tokio::sync::mpsc::Receiver<crate::error::Result<crate::soul::TurnOutcome>>>,
    cli_arc: Option<Arc<tokio::sync::Mutex<Option<crate::app::KimiCLI>>>>,
    turn_cancel_tx: Option<tokio::sync::watch::Sender<bool>>,
    /// Ctrl+C while `running`: first = cancel turn, second = quit shell.
    turn_ctrl_c_during_run: u32,
}

impl std::fmt::Debug for ShellUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellUi")
            .field("messages_len", &self.messages.len())
            .field("scroll_offset", &self.scroll_offset)
            .field("loading", &self.loading)
            .field("exit", &self.exit)
            .finish()
    }
}

impl Default for ShellUi {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            input: InputState::new(),
            scroll_offset: 0,
            loading: false,
            header_model: String::new(),
            permission_label: "Interactive".into(),
            plan_mode: false,
            status: None,
            popup: None,
            modal: None,
            session_dir: PathBuf::new(),
            work_dir: PathBuf::new(),
            exit: false,
            running: false,
            wire_tx: None,
            wire_rx: None,
            outcome_rx: None,
            cli_arc: None,
            turn_cancel_tx: None,
            turn_ctrl_c_during_run: 0,
        }
    }
}

impl ShellUi {
    fn shorten_work_dir(path: &std::path::Path) -> String {
        if let Some(home) = dirs::home_dir() {
            if path.starts_with(&home) {
                let rest = path.strip_prefix(&home).unwrap_or(path);
                if rest.as_os_str().is_empty() {
                    return "~".into();
                }
                return format!("~{}", rest.display());
            }
        }
        path.display().to_string()
    }

    fn header_title(&self) -> String {
        let perm = self
            .status
            .as_ref()
            .map(|s| if s.yolo_enabled { "Yolo" } else { "Interactive" })
            .unwrap_or(self.permission_label.as_str());
        let plan = if self.plan_mode { " · plan" } else { "" };
        if self.header_model.is_empty() {
            format!("🤖 Kimi Code - {perm}{plan}")
        } else {
            format!("🤖 Kimi Code - {} - {perm}{plan}", self.header_model)
        }
    }

    fn ensure_tail_assistant(&mut self) {
        if !matches!(
            self.messages.last().map(|m| m.role),
            Some(ReplMessageRole::Assistant)
        ) {
            self.messages.push(ReplMessage::assistant(String::new()));
        }
    }

    fn append_to_last_assistant(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.ensure_tail_assistant();
        if let Some(last) = self.messages.last_mut() {
            last.text.push_str(chunk);
        }
    }

    fn add_system_message(&mut self, text: impl Into<String>) {
        self.messages.push(ReplMessage::system(text.into()));
    }

    #[inline]
    fn input_area_active(&self) -> bool {
        self.popup.is_none() && self.modal.is_none()
    }

    /// Ctrl+C / Ctrl+Q / SIGINT: idle → exit; running → first cancel turn, second exit.
    fn handle_user_interrupt(&mut self) {
        if self.running {
            self.turn_ctrl_c_during_run = self.turn_ctrl_c_during_run.saturating_add(1);
            if self.turn_ctrl_c_during_run >= 2 {
                self.exit = true;
                return;
            }
            if let Some(ref tx) = self.turn_cancel_tx {
                let _ = tx.send(true);
            }
            self.add_system_message(
                "已请求停止本轮；若仍在输出请稍候。再按一次 Ctrl+C 退出程序。",
            );
            self.scroll_offset = 0;
            return;
        }
        self.exit = true;
    }

    pub async fn run(
        &mut self,
        cli: crate::app::KimiCLI,
        prefill: Option<&str>,
    ) -> crate::error::Result<crate::app::ShellOutcome> {
        self.session_dir = cli.session().dir();
        self.work_dir = cli.session().work_dir.clone();
        self.messages = repl::load_transcript(&self.session_dir).await?;
        if self.messages.is_empty() {
            self.messages.push(repl::welcome_message());
        }

        let soul = cli.soul();
        self.header_model = soul.model_name();
        self.permission_label = if soul.is_yolo() {
            "Yolo".into()
        } else {
            "Interactive".into()
        };
        self.plan_mode = soul.plan_mode;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        // No `EnableMouseCapture`: lets the terminal handle mouse drag-select so transcript is copyable.
        crossterm::execute!(&mut stdout, crossterm::terminal::EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let (wire_tx, wire_rx) = tokio::sync::mpsc::channel::<crate::wire::types::WireMessage>(256);
        self.wire_tx = Some(wire_tx);
        self.wire_rx = Some(wire_rx);
        self.cli_arc = Some(Arc::new(tokio::sync::Mutex::new(Some(cli))));

        if let Some(p) = prefill {
            let p = p.trim();
            if !p.is_empty() {
                self.input.content = p.to_string();
                self.input.cursor_position = self.input.content.len();
            }
        }

        let sigint_exit = Arc::new(AtomicBool::new(false));
        let sigint_flag = sigint_exit.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_ok() {
                    sigint_flag.store(true, Ordering::SeqCst);
                }
            }
        });

        let mut last_tick = Instant::now();
        let tick_rate = Duration::from_millis(250);

        let result = loop {
            if sigint_exit.swap(false, Ordering::AcqRel) {
                self.handle_user_interrupt();
            }
            if self.exit {
                break Ok(crate::app::ShellOutcome::Exit);
            }

            terminal.draw(|f| self.draw(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if crossterm::event::poll(timeout)? {
                if let Some(outcome) = self.handle_event().await {
                    break Ok(outcome);
                }
            }

            self.handle_async_messages().await;

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }
        };

        let to_save: Vec<ReplMessage> = self
            .messages
            .iter()
            .filter(|m| {
                !(m.role == ReplMessageRole::System && m.text.starts_with(repl::WELCOME_TEXT_PREFIX))
            })
            .cloned()
            .collect();
        let _ = repl::save_transcript(&self.session_dir, &to_save).await;

        disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let chunks = draw::main_vertical_layout(frame.area());
        draw::draw_header(frame, chunks[0], &self.header_title());
        draw::draw_messages(frame, chunks[1], &self.messages, self.scroll_offset);
        let bottom = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(1)])
            .split(chunks[2]);
        draw::draw_input(frame, bottom[0], &self.input, self.input_area_active());
        draw::draw_status_footer(
            frame,
            bottom[1],
            &self.header_model,
            &Self::shorten_work_dir(&self.work_dir),
            self.loading,
        );

        if self.modal.is_some() {
            let area = centered_rect_percent(60, 40, frame.area());
            frame.render_widget(ratatui::widgets::Clear, area);
            self.draw_modal(frame, area);
        }

        if let Some(ref popup) = self.popup {
            let area = centered_rect_percent(70, 50, frame.area());
            frame.render_widget(ratatui::widgets::Clear, area);
            self.draw_popup(frame, area, popup);
        }
    }

    async fn handle_event(&mut self) -> Option<crate::app::ShellOutcome> {
        if let Ok(event) = crossterm::event::read() {
            match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    return self.handle_key(key).await;
                }
                Event::Paste(text) => {
                    if self.input_area_active() {
                        self.input.insert_str(&text);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        None
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Option<crate::app::ShellOutcome> {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_user_interrupt();
                return None;
            }
            _ => {}
        }

        if self.popup.is_some() {
            return self.handle_popup_key(key).await;
        }

        if self.modal.is_some() {
            let wire_tx = self.wire_tx.clone().expect("wire_tx");
            return self.handle_modal_key(key, &wire_tx).await;
        }

        if key.code == KeyCode::Esc && self.loading {
            if let Some(ref tx) = self.turn_cancel_tx {
                let _ = tx.send(true);
            }
            self.outcome_rx = None;
            self.turn_cancel_tx = None;
            self.loading = false;
            self.running = false;
            self.turn_ctrl_c_during_run = 0;
            self.add_system_message("已取消（Esc）");
            return None;
        }

        if key.code == KeyCode::Esc {
            self.exit = true;
            return None;
        }

        match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match c {
                    'h' => self.popup = Some(Popup::Help),
                    'r' => {
                        let events = self.gather_replay_events().await;
                        self.popup = Some(Popup::Replay(events, 0));
                    }
                    's' => {
                        let sessions = self.gather_sessions().await;
                        self.popup = Some(Popup::SessionPicker(sessions, 0));
                    }
                    't' => {
                        let tasks = self.gather_tasks().await;
                        self.popup = Some(Popup::TaskList(tasks, 0));
                    }
                    _ => {}
                }
                None
            }
            KeyCode::Char(c) => {
                self.input.insert_char(c);
                None
            }
            KeyCode::Backspace => {
                self.input.backspace();
                None
            }
            KeyCode::Delete => {
                self.input.delete();
                None
            }
            KeyCode::Left => {
                self.input.move_left();
                None
            }
            KeyCode::Right => {
                self.input.move_right();
                None
            }
            KeyCode::Home => {
                self.input.move_home();
                None
            }
            KeyCode::End => {
                self.input.move_end();
                None
            }
            KeyCode::Up => {
                self.input.history_prev();
                None
            }
            KeyCode::Down => {
                self.input.history_next();
                None
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                None
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            KeyCode::Enter => self.submit_input().await,
            _ => None,
        }
    }

    async fn submit_input(&mut self) -> Option<crate::app::ShellOutcome> {
        if self.running {
            return None;
        }
        let text = self.input.submit();
        if text.is_empty() {
            return None;
        }

        if let Some(outcome) = self.handle_shell_slash_command(&text) {
            self.scroll_offset = 0;
            return Some(outcome);
        }

        self.messages.push(ReplMessage::user(text.clone()));
        self.messages.push(ReplMessage::assistant(String::new()));
        self.scroll_offset = 0;

        self.loading = true;
        self.running = true;
        self.turn_ctrl_c_during_run = 0;

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        self.turn_cancel_tx = Some(cancel_tx);

        let parts = vec![crate::soul::message::ContentPart::Text { text }];
        let cli_arc = self.cli_arc.clone().expect("cli_arc");
        let wire_tx = self.wire_tx.clone().expect("wire_tx");
        let (outcome_tx, outcome_rx) = tokio::sync::mpsc::channel(1);
        self.outcome_rx = Some(outcome_rx);

        tokio::spawn(async move {
            let mut cli = cli_arc.lock().await.take().expect("cli");
            let result = cli
                .run_with_wire(
                    parts,
                    {
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
                    },
                    Some(cancel_rx),
                )
                .await;
            let _ = outcome_tx.send(result).await;
            *cli_arc.lock().await = Some(cli);
        });

        None
    }

    async fn handle_async_messages(&mut self) {
        if let Some(mut rx) = self.wire_rx.take() {
            while let Ok(msg) = rx.try_recv() {
                self.handle_wire_message(msg).await;
            }
            self.wire_rx = Some(rx);
        }

        if let Some(ref mut rx) = self.outcome_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.running = false;
                    self.loading = false;
                    self.turn_ctrl_c_during_run = 0;
                    self.outcome_rx = None;
                    self.turn_cancel_tx = None;
                    self.modal = None;

                    match result {
                        Ok(outcome) => {
                            if outcome.stop_reason == crate::soul::TurnStopReason::Cancelled {
                                self.add_system_message("Turn cancelled.");
                            }
                            if let Some(msg) = outcome.final_message {
                                let fm = msg.extract_text("");
                                if !fm.is_empty() {
                                    if let Some(last) = self.messages.last_mut() {
                                        if last.role == ReplMessageRole::Assistant && last.text.trim().is_empty() {
                                            last.text = fm;
                                        } else if !last.text.contains(&fm) {
                                            self.append_to_last_assistant("\n\n");
                                            self.append_to_last_assistant(&fm);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            self.add_system_message(&format!("{e}"));
                        }
                    }
                    self.scroll_offset = 0;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    self.running = false;
                    self.loading = false;
                    self.turn_ctrl_c_during_run = 0;
                    self.outcome_rx = None;
                    self.turn_cancel_tx = None;
                    self.add_system_message("Turn finished but the result channel closed unexpectedly.");
                    self.scroll_offset = 0;
                }
            }
        }
    }

    async fn handle_wire_message(&mut self, msg: crate::wire::types::WireMessage) {
        match msg {
            crate::wire::types::WireMessage::StepBegin { step_no } => {
                self.append_to_last_assistant(&format!("\n--- step {step_no} ---\n"));
            }
            crate::wire::types::WireMessage::ToolCall { name, arguments, .. } => {
                let arg = crate::tools::extract_key_argument(&arguments.to_string(), &name);
                let line = if let Some(a) = arg {
                    format!("\n[tool {name} ({a})]\n")
                } else {
                    format!("\n[tool {name}]\n")
                };
                self.append_to_last_assistant(&line);
            }
            crate::wire::types::WireMessage::ToolResult { result, .. } => {
                let t = result.extract_text();
                let preview: String = t.chars().take(400).collect();
                self.append_to_last_assistant(&format!("→ {preview}\n"));
            }
            crate::wire::types::WireMessage::TextPart { text } => {
                self.append_to_last_assistant(&text);
            }
            crate::wire::types::WireMessage::ThinkPart { thought } => {
                self.append_to_last_assistant(&format!("[think] {thought}"));
            }
            crate::wire::types::WireMessage::Notification { text } => {
                self.add_system_message(text);
            }
            crate::wire::types::WireMessage::McpLoadingBegin => {
                self.append_to_last_assistant("\n[MCP loading…]\n");
            }
            crate::wire::types::WireMessage::McpLoadingEnd => {
                self.append_to_last_assistant("\n[MCP ready]\n");
            }
            crate::wire::types::WireMessage::CompactionBegin => {
                self.append_to_last_assistant("\n[compacting…]\n");
            }
            crate::wire::types::WireMessage::CompactionEnd => {
                self.append_to_last_assistant("\n[compaction done]\n");
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
                self.modal = Some(Modal::Approval {
                    request_id: id,
                    _tool_call_id: tool_call_id,
                    sender,
                    action,
                    description,
                    selected_index: 0,
                });
            }
            crate::wire::types::WireMessage::QuestionRequest { id, items } => {
                self.modal = Some(Modal::Question {
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

    fn handle_shell_slash_command(&mut self, text: &str) -> Option<crate::app::ShellOutcome> {
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
                "login" | "setup" => {
                    match std::process::Command::new("kimi").arg("login").status() {
                        Ok(s) if s.success() => {
                            return Some(crate::app::ShellOutcome::Reload {
                                session_id: None,
                                prefill_text: None,
                            });
                        }
                        Ok(s) => self.add_system_message(format!("Login failed with status: {s}")),
                        Err(e) => self.add_system_message(format!("Failed to run `kimi login`: {e}")),
                    }
                    return None;
                }
                "logout" => {
                    match std::process::Command::new("kimi").arg("logout").status() {
                        Ok(s) if s.success() => {
                            return Some(crate::app::ShellOutcome::Reload {
                                session_id: None,
                                prefill_text: None,
                            });
                        }
                        Ok(s) => self.add_system_message(format!("Logout failed with status: {s}")),
                        Err(e) => self.add_system_message(format!("Failed to run `kimi logout`: {e}")),
                    }
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    async fn handle_modal_key(
        &mut self,
        key: KeyEvent,
        wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>,
    ) -> Option<crate::app::ShellOutcome> {
        match &mut self.modal {
            Some(Modal::Approval { selected_index, .. }) => match key.code {
                KeyCode::Up => {
                    *selected_index = selected_index.saturating_sub(1);
                    None
                }
                KeyCode::Down => {
                    *selected_index = (*selected_index + 1).min(3);
                    None
                }
                KeyCode::Char('1') => {
                    *selected_index = 0;
                    None
                }
                KeyCode::Char('2') => {
                    *selected_index = 1;
                    None
                }
                KeyCode::Char('3') => {
                    *selected_index = 2;
                    None
                }
                KeyCode::Char('4') => {
                    *selected_index = 3;
                    None
                }
                KeyCode::Enter => {
                    self.submit_approval(wire_tx).await;
                    None
                }
                KeyCode::Esc => {
                    self.cancel_modal(wire_tx).await;
                    None
                }
                _ => None,
            },
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
                        self.submit_question(wire_tx).await;
                        None
                    }
                    KeyCode::Esc => {
                        self.cancel_modal(wire_tx).await;
                        None
                    }
                    _ => None,
                }
            }
            None => None,
        }
    }

    async fn submit_approval(&mut self, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Approval { request_id, selected_index, .. }) = self.modal.take() {
            let response_str = match selected_index {
                0 => "approve",
                1 => "approve_for_session",
                _ => "reject",
            }
            .to_string();
            let _ = wire_tx
                .send(crate::wire::types::WireMessage::ApprovalResponse {
                    request_id,
                    response: response_str,
                    feedback: None,
                })
                .await;
        }
    }

    async fn submit_question(&mut self, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Question { request_id, answers, .. }) = self.modal.take() {
            let _ = wire_tx
                .send(crate::wire::types::WireMessage::QuestionResponse {
                    request_id,
                    answers: answers.clone(),
                })
                .await;
        }
    }

    async fn cancel_modal(&mut self, wire_tx: &tokio::sync::mpsc::Sender<crate::wire::types::WireMessage>) {
        if let Some(Modal::Approval { request_id, .. }) = self.modal.take() {
            let _ = wire_tx
                .send(crate::wire::types::WireMessage::ApprovalResponse {
                    request_id,
                    response: "reject".into(),
                    feedback: None,
                })
                .await;
        } else if let Some(Modal::Question { request_id, .. }) = self.modal.take() {
            let _ = wire_tx
                .send(crate::wire::types::WireMessage::QuestionResponse {
                    request_id: request_id.clone(),
                    answers: HashMap::new(),
                })
                .await;
        }
    }

    async fn handle_popup_key(&mut self, key: KeyEvent) -> Option<crate::app::ShellOutcome> {
        match &mut self.popup {
            Some(Popup::Help) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') => self.popup = None,
                    _ => {}
                }
                None
            }
            Some(Popup::Replay(events, selected)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('r') => self.popup = None,
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => {
                        if *selected + 1 < events.len() {
                            *selected += 1;
                        }
                    }
                    _ => {}
                }
                None
            }
            Some(Popup::SessionPicker(sessions, selected)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => self.popup = None,
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => {
                        if *selected + 1 < sessions.len() {
                            *selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some((session_id, _)) = sessions.get(*selected) {
                            if session_id != "__empty__" {
                                self.exit = true;
                                return Some(crate::app::ShellOutcome::Reload {
                                    session_id: Some(session_id.clone()),
                                    prefill_text: None,
                                });
                            }
                        }
                        self.popup = None;
                    }
                    _ => {}
                }
                None
            }
            Some(Popup::TaskList(_, _)) => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => self.popup = None,
                    _ => {}
                }
                None
            }
            None => None,
        }
    }

    async fn gather_replay_events(&self) -> Vec<String> {
        let Some(ref cli_arc) = self.cli_arc else {
            return vec!["No CLI available.".into()];
        };
        let cli_guard = cli_arc.lock().await;
        let Some(cli) = cli_guard.as_ref() else {
            return vec!["No CLI available.".into()];
        };
        let records = cli.soul().wire_file().records();
        let mut events = Vec::new();
        for record in records.into_iter().rev().take(20) {
            let text = match record {
                crate::wire::types::WireMessage::TextPart { text } => {
                    format!("Text: {}", text.chars().take(60).collect::<String>())
                }
                crate::wire::types::WireMessage::ThinkPart { thought } => {
                    format!("Think: {}", thought.chars().take(60).collect::<String>())
                }
                crate::wire::types::WireMessage::ToolCall { name, .. } => format!("ToolCall: {name}"),
                crate::wire::types::WireMessage::ToolResult { result, .. } => {
                    format!("ToolResult: {}", result.extract_text().chars().take(60).collect::<String>())
                }
                crate::wire::types::WireMessage::StepBegin { step_no } => format!("Step {step_no}"),
                crate::wire::types::WireMessage::TurnBegin { .. } => "Turn begin".into(),
                crate::wire::types::WireMessage::Notification { text } => format!("Notify: {text}"),
                _ => format!("{record:?}"),
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

    async fn gather_tasks(&self) -> Vec<String> {
        let Some(ref cli_arc) = self.cli_arc else {
            return vec!["No CLI available.".into()];
        };
        let cli_guard = cli_arc.lock().await;
        let Some(cli) = cli_guard.as_ref() else {
            return vec!["No CLI available.".into()];
        };
        let tasks = cli.soul().runtime.background_tasks.list(false).await;
        if tasks.is_empty() {
            return vec!["No background tasks.".into()];
        }
        let mut result = Vec::new();
        for t in tasks {
            let status = if t.is_running().await { "running" } else { "done" };
            result.push(format!("[{}] {} · {}", status, t.command, t.id));
        }
        result
    }

    fn draw_popup(&self, frame: &mut ratatui::Frame, area: Rect, popup: &Popup) {
        match popup {
            Popup::Help => {
                let lines = vec![
                    Line::from(vec![Span::styled(
                        "Shortcuts",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from("Ctrl+C / Ctrl+Q  空闲：退出 · 助手运行时：第一次取消本轮，第二次退出"),
                    Line::from("Esc            助手运行时：取消本轮 · 空闲：退出"),
                    Line::from("Mouse drag     在 Messages 里拖选复制（未启用鼠标捕获）"),
                    Line::from("PgUp / PgDn    上下滚动对话"),
                    Line::from("Ctrl+H         Help"),
                    Line::from("Ctrl+R         Replay recent events"),
                    Line::from("Ctrl+S         Session picker"),
                    Line::from("Ctrl+T         Task browser"),
                    Line::from("Up/Down        Input history"),
                    Line::from("运行中仍可编辑输入框；须等本轮结束后再按 Enter 发送"),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Press Esc or H to close",
                        Style::default().fg(Color::DarkGray),
                    )]),
                ];
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(
                        Block::default()
                            .title("Help")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
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
                    .block(
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
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
                    .block(
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
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
                    .block(
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
        }
    }

    fn draw_modal(&self, frame: &mut ratatui::Frame, area: Rect) {
        match &self.modal {
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
                    Line::from(vec![Span::styled(
                        "Approval request",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(format!("{sender} wants to {action}:")),
                    Line::from(description.as_str()),
                    Line::from(""),
                ];
                for (i, opt) in options.iter().enumerate() {
                    let num = i + 1;
                    if i == *selected_index {
                        lines.push(Line::from(vec![
                            Span::styled("> ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format!("[{num}] {opt}"),
                                Style::default().fg(Color::Cyan),
                            ),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(
                                format!("[{num}] {opt}"),
                                Style::default().fg(Color::Gray),
                            ),
                        ]));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "1-4 choose, Enter confirm, Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )]));
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(
                        Block::default()
                            .title("Approval")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    )
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
                let mut lines = vec![Line::from(vec![Span::styled(
                    format!("? {}", q.question),
                    Style::default().fg(Color::Yellow),
                )])];
                if q.multi_select {
                    lines.push(Line::from("(SPACE to toggle, ENTER to submit)"));
                }
                lines.push(Line::from(""));
                let option_count = q.options.len() + 1;
                for (i, opt) in q.options.iter().enumerate() {
                    let prefix = if q.multi_select {
                        if multi_selected.contains(&i) {
                            "[x] "
                        } else {
                            "[ ] "
                        }
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
                        Span::styled(format!("{prefix}{}", opt.label), style),
                        Span::styled(
                            format!(" - {}", opt.description),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }
                let other_idx = option_count - 1;
                let prefix = if q.multi_select {
                    if multi_selected.contains(&other_idx) {
                        "[x] "
                    } else {
                        "[ ] "
                    }
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
                lines.push(Line::from(vec![Span::styled(format!("{}Other", prefix), style)]));
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Enter submit, Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )]));
                let paragraph = Paragraph::new(Text::from(lines))
                    .block(
                        Block::default()
                            .title("Question")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Gray)),
                    )
                    .wrap(Wrap { trim: true });
                frame.render_widget(paragraph, area);
            }
            None => {}
        }
    }
}

fn centered_rect_percent(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_ui_default() {
        let ui = ShellUi::default();
        assert!(ui.messages.is_empty());
    }

    #[test]
    fn welcome_message_ported() {
        let w = repl::welcome_message();
        assert!(w.text.starts_with(repl::WELCOME_TEXT_PREFIX));
    }
}
