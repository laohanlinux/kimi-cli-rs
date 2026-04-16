use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::io;

/// A single turn in the shell history.
#[derive(Debug, Clone)]
struct HistoryItem {
    role: &'static str,
    content: String,
}

/// Interactive shell UI using ratatui.
#[derive(Debug, Clone, Default)]
pub struct ShellUi {
    history: Vec<HistoryItem>,
    input: String,
    scroll_offset: u16,
}

impl ShellUi {
    pub async fn run(
        &mut self,
        cli: &mut crate::app::KimiCLI,
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
        cli: &mut crate::app::KimiCLI,
    ) -> crate::error::Result<crate::app::ShellOutcome> {
        loop {
            terminal.draw(|f| self.draw(f))?;

            let event = match tokio::task::spawn_blocking(|| crossterm::event::read()).await {
                Ok(Ok(evt)) => evt,
                _ => continue,
            };

            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(outcome) = self.handle_key(key, cli).await? {
                    break Ok(outcome);
                }
            } else if let Event::Resize(_, _) = event {
                // Redraw automatically on next loop iteration
            }
        }
    }

    /// Handles a key press. Returns `Some(outcome)` if the shell should exit.
    async fn handle_key(
        &mut self,
        key: KeyEvent,
        cli: &mut crate::app::KimiCLI,
    ) -> crate::error::Result<Option<crate::app::ShellOutcome>> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Some(crate::app::ShellOutcome::Exit))
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Some(crate::app::ShellOutcome::Exit))
            }
            KeyCode::Esc => return Ok(Some(crate::app::ShellOutcome::Exit)),
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return Ok(None);
                }

                // Handle control-flow slash commands locally before sending to soul.
                if let Some(outcome) = self.handle_shell_slash_command(&text, cli) {
                    self.input.clear();
                    self.scroll_offset = 0;
                    return Ok(Some(outcome));
                }

                self.history.push(HistoryItem {
                    role: "user",
                    content: text.clone(),
                });
                self.input.clear();
                self.scroll_offset = 0;

                let parts = vec![crate::soul::message::ContentPart::Text { text }];
                let outcome = cli.run(parts).await?;
                let reply = outcome
                    .final_message
                    .map(|m| m.extract_text(""))
                    .unwrap_or_else(|| "(no response yet — soul stub)".into());
                self.history.push(HistoryItem {
                    role: "assistant",
                    content: reply,
                });
                Ok(None)
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                Ok(None)
            }
            KeyCode::Backspace => {
                self.input.pop();
                Ok(None)
            }
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                Ok(None)
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Intercepts shell-local slash commands that affect control flow.
    fn handle_shell_slash_command(
        &mut self,
        text: &str,
        cli: &crate::app::KimiCLI,
    ) -> Option<crate::app::ShellOutcome> {
        if let Some(cmd_text) = text.strip_prefix('/') {
            let parts: Vec<&str> = cmd_text.splitn(2, ' ').collect();
            match parts[0] {
                "web" => {
                    return Some(crate::app::ShellOutcome::SwitchToWeb {
                        session_id: Some(cli.session().id.clone()),
                    })
                }
                "vis" => {
                    return Some(crate::app::ShellOutcome::SwitchToVis {
                        session_id: Some(cli.session().id.clone()),
                    })
                }
                "reload" => {
                    let prefill = parts.get(1).map(|s| s.to_string());
                    return Some(crate::app::ShellOutcome::Reload {
                        session_id: Some(cli.session().id.clone()),
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

    fn draw(&self, frame: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(frame.area());

        let history_text = self
            .history
            .iter()
            .flat_map(|item| {
                let color = match item.role {
                    "user" => Color::Cyan,
                    "assistant" => Color::Green,
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

        let input_text = format!("> {}", self.input);
        let input_paragraph = Paragraph::new(input_text)
            .block(Block::default().title("Input (Enter=send, Ctrl+C/Esc=quit)").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        frame.render_widget(input_paragraph, chunks[1]);

        // Set cursor position inside input box
        let input_area = chunks[1];
        let cursor_x = (input_area.x + 2 + self.input.len() as u16).min(input_area.x + input_area.width - 1);
        let cursor_y = input_area.y + 1;
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));
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
