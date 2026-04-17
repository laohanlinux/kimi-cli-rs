//! Ported from `claude-code-rs/src/repl/mod.rs` (layout + widgets only).

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use super::input_state::InputState;
use super::message::{ReplMessage, ReplMessageRole};

/// Assistant label in the transcript (Claude uses "Claude").
pub const ASSISTANT_LABEL: &str = "Kimi";

/// Transient welcome prefix — excluded from persistence (see Claude `ReplApp::run` filter).
pub const WELCOME_TEXT_PREFIX: &str = "🤖 Welcome to Kimi Code";

pub fn welcome_message() -> ReplMessage {
    ReplMessage::system(
        "🤖 Welcome to Kimi Code!\n\n\
         Type your message and press Enter to chat.\n\
         Type /help for available commands.\n\
         Press Ctrl+C or type /exit to quit.",
    )
}

pub fn draw_header(frame: &mut Frame, area: Rect, title: &str) {
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

pub fn draw_messages(frame: &mut Frame, area: Rect, messages: &[ReplMessage], scroll_offset: usize) {
    let mut lines: Vec<Line> = Vec::new();

    for message in messages {
        match message.role {
            ReplMessageRole::User => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "You",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                )]));
                for line in message.text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
            ReplMessageRole::Assistant => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    ASSISTANT_LABEL,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )]));
                for line in message.text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
            ReplMessageRole::System => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "System",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )]));
                for line in message.text.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }
    }

    let text = Text::from(lines);
    let messages_widget = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .scroll((scroll_offset as u16, 0))
        .block(Block::default().borders(Borders::ALL).title("Messages"));
    frame.render_widget(messages_widget, area);

    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state = ScrollbarState::new(messages.len()).position(scroll_offset);
    let inner_area = area.inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    frame.render_stateful_widget(scrollbar, inner_area, &mut scrollbar_state);
}

/// `active` = main input accepts keys (false when a popup/modal has focus — still drawn dimmed).
pub fn draw_input(frame: &mut Frame, area: Rect, input: &InputState, active: bool) {
    let input_style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input_text = if input.content.is_empty() && active {
        "输入…  Enter 发送 · Ctrl+C 停轮/退出".to_string()
    } else if input.content.is_empty() {
        String::new()
    } else {
        input.content.clone()
    };

    let input_widget = Paragraph::new(input_text)
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title("Input"));

    frame.render_widget(input_widget, area);

    if active {
        let cursor_x = area.x + input.cursor_position as u16 + 1;
        let cursor_y = area.y + 1;
        frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));
    }
}

pub fn main_vertical_layout(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            // Input (3) + single-line footer (1).
            Constraint::Length(4),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

/// Compact footer: model, cwd, hints (`running` does not block typing — only Enter submit).
pub fn draw_status_footer(
    frame: &mut Frame,
    area: Rect,
    model: &str,
    work_dir: &str,
    loading: bool,
) {
    let model_s = if model.is_empty() { "—" } else { model };
    let run = if loading { " │ busy" } else { "" };
    let line = Line::from(vec![
        Span::styled(model_s, Style::default().fg(Color::Cyan)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(work_dir, Style::default().fg(Color::Green)),
        Span::styled(run, Style::default().fg(Color::Yellow)),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("/help Ctrl+H · PgUp/PgDn scroll", Style::default().fg(Color::DarkGray)),
    ]);
    let w = Paragraph::new(line);
    frame.render_widget(w, area);
}
