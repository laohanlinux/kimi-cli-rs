//! Ported from `claude-code-rs/src/components/input/mod.rs` (InputState + line editor).

/// Input state (Claude Code REPL).
#[derive(Debug, Default)]
pub struct InputState {
    pub content: String,
    pub cursor_position: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.content.remove(self.cursor_position);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_position < self.content.len() {
            self.content.remove(self.cursor_position);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_position = self.content.len();
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor_position = 0;
    }

    pub fn submit(&mut self) -> String {
        let content = self.content.clone();
        if !content.is_empty() {
            self.history.push(content.clone());
        }
        self.clear();
        self.history_index = None;
        content
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            Some(i) if i > 0 => i - 1,
            None => self.history.len() - 1,
            Some(_) => return,
        };

        self.history_index = Some(new_index);
        self.content = self.history[new_index].clone();
        self.cursor_position = self.content.len();
    }

    pub fn history_next(&mut self) {
        let new_index = match self.history_index {
            Some(i) if i < self.history.len() - 1 => i + 1,
            Some(_) => {
                self.history_index = None;
                self.clear();
                return;
            }
            None => return,
        };

        self.history_index = Some(new_index);
        self.content = self.history[new_index].clone();
        self.cursor_position = self.content.len();
    }
}
