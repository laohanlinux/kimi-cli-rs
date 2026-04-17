pub mod broadcast;
pub mod changelog;
pub mod clipboard;
pub mod datetime;
pub mod diff;
pub mod editor;
pub mod environment;
pub mod envvar;
pub mod export;
pub mod file_filter;
pub mod frontmatter;
pub mod io;
pub mod media_tags;
pub mod message;
pub mod path;
pub mod proxy;
pub mod rich;
pub mod sensitive;
pub mod server;
pub mod signals;
pub mod slashcmd;
pub mod string;
pub mod subprocess_env;

pub use string::{random_string, shorten, shorten_middle};

/// Sanitizes a CLI path string.
pub fn sanitize_cli_path(path: &str) -> String {
    path.trim().to_string()
}
