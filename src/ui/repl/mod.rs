//! Claude Code REPL building blocks (ported from `claude-code-rs/src/repl` + `components/input`).

pub mod draw;
pub mod input_state;
pub mod message;
pub mod storage;

pub use draw::{
    WELCOME_TEXT_PREFIX, draw_header, draw_input, draw_messages, draw_status_footer,
    main_vertical_layout, welcome_message,
};
pub use input_state::InputState;
pub use message::{ReplMessage, ReplMessageRole};
pub use storage::{load_transcript, save_transcript, transcript_path};
