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
    ) -> crate::error::Result<()> {
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
    ) -> crate::error::Result<()> {
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
                if self.handle_key(key, cli).await? {
                    break Ok(());
                }
            } else if let Event::Resize(_, _) = event {
                // Redraw automatically on next loop iteration
            }
        }
    }

    /// Handles a key press. Returns `true` if the shell should exit.
    async fn handle_key(
        &mut self,
        key: KeyEvent,
        cli: &mut crate::app::KimiCLI,
    ) -> crate::error::Result<bool> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            KeyCode::Esc => return Ok(true),
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return Ok(false);
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
                Ok(false)
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                Ok(false)
            }
            KeyCode::Backspace => {
                self.input.pop();
                Ok(false)
            }
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                Ok(false)
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                Ok(false)
            }
            _ => Ok(false),
        }
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
