use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use std::time::Instant;

/// Estimates token count for mixed CJK/Latin text.
/// Heuristic: CJK ~1.5 tokens/char, Latin ~1 token per 4 chars.
pub fn estimate_tokens(text: &str) -> f64 {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        let cp = ch as u32;
        if (0x4E00..=0x9FFF).contains(&cp)
            || (0x3400..=0x4DBF).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp)
            || (0x3000..=0x303F).contains(&cp)
            || (0xFF00..=0xFFEF).contains(&cp)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk as f64 * 1.5 + other as f64 / 4.0
}

/// Formats a token count for display.
pub fn format_token_count(count: usize) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Streaming content block with token tracking and composing indicator.
#[derive(Debug, Clone)]
pub struct ContentBlock {
    is_think: bool,
    raw_text: String,
    token_count: f64,
    start_time: Instant,
}

impl ContentBlock {
    pub fn new(is_think: bool) -> Self {
        Self {
            is_think,
            raw_text: String::new(),
            token_count: 0.0,
            start_time: Instant::now(),
        }
    }

    pub fn append(&mut self, text: &str) {
        self.raw_text.push_str(text);
        self.token_count += estimate_tokens(text);
    }

    pub fn is_empty(&self) -> bool {
        self.raw_text.is_empty()
    }

    pub fn text(&self) -> &str {
        &self.raw_text
    }

    pub fn clear(&mut self) {
        self.raw_text.clear();
        self.token_count = 0.0;
        self.start_time = Instant::now();
    }

    /// Renders the block as lines for the live area.
    pub fn render(&self) -> Vec<Line<'_>> {
        let elapsed = self.start_time.elapsed().as_secs();
        let elapsed_str = format!("{}s", elapsed);
        let count_str = format_token_count(self.token_count as usize);

        let label = if self.is_think {
            "Thinking"
        } else {
            "Composing"
        };

        let indicator = vec![
            Span::styled(
                label,
                Style::default()
                    .fg(if self.is_think { Color::Magenta } else { Color::Green }),
            ),
            Span::styled(format!(" {elapsed_str}"), Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(" · {count_str} tokens"),
                Style::default().fg(Color::DarkGray),
            ),
        ];

        let mut lines = vec![Line::from(indicator)];
        if !self.raw_text.is_empty() {
            // Show a preview of the latest content (last 4 lines).
            let preview: String = self
                .raw_text
                .lines()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            if self.is_think {
                lines.push(Line::from(Span::styled(
                    preview,
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(preview));
            }
        }
        lines
    }
}

/// Block representing a single tool call and its result.
#[derive(Debug, Clone)]
pub struct ToolCallBlock {
    pub tool_name: String,
    pub argument: Option<String>,
    pub result_text: Option<String>,
    pub is_error: bool,
    pub finished: bool,
}

impl ToolCallBlock {
    pub fn new(tool_name: impl Into<String>, argument: Option<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            argument,
            result_text: None,
            is_error: false,
            finished: false,
        }
    }

    pub fn finish(&mut self, result: &crate::soul::message::ToolReturnValue) {
        self.finished = true;
        match result {
            crate::soul::message::ToolReturnValue::Ok { output, message } => {
                self.result_text = Some(
                    message
                        .clone()
                        .unwrap_or_else(|| output.trim().to_string()),
                );
                self.is_error = false;
            }
            crate::soul::message::ToolReturnValue::Error { error } => {
                self.result_text = Some(error.clone());
                self.is_error = true;
            }
            crate::soul::message::ToolReturnValue::Parts { parts } => {
                let text = parts
                    .iter()
                    .map(|p| match p {
                        crate::soul::message::ContentPart::Text { text } => text.as_str(),
                        crate::soul::message::ContentPart::Think { thought } => thought.as_str(),
                        crate::soul::message::ContentPart::ImageUrl { url } => url.as_str(),
                        crate::soul::message::ContentPart::AudioUrl { url } => url.as_str(),
                        crate::soul::message::ContentPart::VideoUrl { url } => url.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                self.result_text = Some(text.trim().to_string());
                self.is_error = false;
            }
        }
    }

    pub fn render(&self) -> Vec<Line<'_>> {
        let mut spans = vec![
            Span::styled(
                if self.finished { "Used " } else { "Using " },
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(&self.tool_name, Style::default().fg(Color::Blue)),
        ];
        if let Some(arg) = &self.argument {
            spans.push(Span::styled(" (", Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled(arg, Style::default().fg(Color::DarkGray)));
            spans.push(Span::styled(")", Style::default().fg(Color::DarkGray)));
        }
        let mut lines = vec![Line::from(spans)];
        if self.finished {
            if let Some(result) = &self.result_text {
                let style = if self.is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };
                // Truncate very long results for the live area.
                let preview = if result.len() > 200 {
                    format!("{}...", &result[..200])
                } else {
                    result.clone()
                };
                lines.push(Line::from(vec![
                    Span::styled("→ ", style),
                    Span::styled(preview, style),
                ]));
            }
        }
        lines
    }
}

/// Notification block with severity-based styling.
#[derive(Debug, Clone)]
pub struct NotificationBlock {
    pub title: String,
    pub body: String,
    pub severity: String,
}

impl NotificationBlock {
    pub fn new(title: impl Into<String>, body: impl Into<String>, severity: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            severity: severity.into(),
        }
    }

    fn severity_color(&self) -> Color {
        match self.severity.as_str() {
            "success" => Color::Green,
            "warning" => Color::Yellow,
            "error" => Color::Red,
            _ => Color::Cyan,
        }
    }

    pub fn render(&self) -> Vec<Line<'_>> {
        let color = self.severity_color();
        let mut lines = vec![Line::from(Span::styled(
            &self.title,
            Style::default()
                .fg(color)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ))];
        if !self.body.is_empty() {
            let body_lines: Vec<&str> = self.body.lines().collect();
            let preview = if body_lines.len() > 2 {
                format!("{}\n...", body_lines[..2].join("\n"))
            } else {
                self.body.clone()
            };
            lines.push(Line::from(Span::styled(preview, Style::default().fg(Color::DarkGray))));
        }
        lines
    }
}

/// Status block for the bottom status bar.
#[derive(Debug, Clone, Default)]
pub struct StatusBlock {
    pub context_usage: f64,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
    pub yolo_enabled: bool,
    pub plan_mode: bool,
    pub mcp_connected: usize,
    pub mcp_total: usize,
    pub mcp_tools: usize,
    pub has_mcp: bool,
    pub generating: bool,
}

impl StatusBlock {
    pub fn render(&self) -> Text<'_> {
        let mut spans = vec![];
        let status_str = crate::soul::format_context_status(
            self.context_usage,
            self.context_tokens,
            self.max_context_tokens,
        );
        spans.push(Span::raw(status_str));
        if self.yolo_enabled {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("YOLO", Style::default().fg(Color::Red)));
        }
        if self.plan_mode {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("PLAN", Style::default().fg(Color::Blue)));
        }
        if self.has_mcp {
            spans.push(Span::raw(" | "));
            spans.push(Span::raw(format!(
                "MCP {}/{} conn, {} tools",
                self.mcp_connected, self.mcp_total, self.mcp_tools
            )));
        }
        if self.generating {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("Generating...", Style::default().fg(Color::Green)));
        }
        Text::from(Line::from(spans))
    }
}

/// Renders a centered popup panel inside the given area.
pub fn render_popup_panel<'a>(
    title: &'a str,
    content: Text<'a>,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let popup_area = centered_rect(60, 60, area);
    Clear.render(popup_area, buf);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });
    paragraph.render(popup_area, buf);
}

/// Renders a list of items inside a popup panel.
pub fn render_popup_list(
    title: &str,
    items: Vec<String>,
    selected: Option<usize>,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let popup_area = centered_rect(60, 60, area);
    Clear.render(popup_area, buf);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let lines: Vec<Line> = items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if Some(i) == selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            Line::from(Span::styled(item, style))
        })
        .collect();
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: true });
    paragraph.render(popup_area, buf);
}

/// Computes a centered rectangle with the given percentage size.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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
    fn content_block_tracks_tokens() {
        let mut block = ContentBlock::new(false);
        block.append("hello world");
        assert!(!block.is_empty());
        assert!(block.token_count > 0.0);
    }

    #[test]
    fn tool_call_block_renders_finished() {
        let mut block = ToolCallBlock::new("FetchURL", Some("https://example.com".into()));
        block.finish(&crate::soul::message::ToolReturnValue::Ok {
            output: "test".into(),
            message: None,
        });
        let lines = block.render();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn notification_block_severity_colors() {
        let block = NotificationBlock::new("Test", "Body", "error");
        assert_eq!(block.severity_color(), Color::Red);
    }
}
