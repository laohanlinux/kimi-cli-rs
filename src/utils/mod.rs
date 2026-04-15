pub mod environment;
pub mod path;

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
