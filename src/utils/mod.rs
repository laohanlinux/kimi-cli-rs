pub mod clipboard;
pub mod diff;
pub mod editor;
pub mod environment;
pub mod io;
pub mod path;
pub mod string;

/// Shortens a string by truncating from the end with an ellipsis.
pub fn shorten(text: &str, width: usize) -> String {
    if text.len() <= width {
        return text.to_string();
    }
    format!("{}...", &text[..width.saturating_sub(3)])
}

/// Shortens a string by keeping the middle.
pub fn shorten_middle(text: &str, width: usize) -> String {
    if text.len() <= width {
        return text.to_string();
    }
    let half = width / 2;
    format!("{}...{}", &text[..half], &text[text.len() - half..])
}

/// Sanitizes a CLI path string.
pub fn sanitize_cli_path(path: &str) -> String {
    path.trim().to_string()
}
