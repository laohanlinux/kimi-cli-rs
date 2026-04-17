/// Shortens a string to the given maximum length, preserving the middle.
pub fn shorten_middle(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let keep = max_len.saturating_sub(3) / 2;
    format!("{}...{}", &s[..keep], &s[s.len().saturating_sub(keep)..])
}

/// Counts visible characters (grapheme clusters) in a string.
pub fn visible_len(s: &str) -> usize {
    s.chars().count()
}

/// Truncates a string to the given maximum visible length with an ellipsis.
pub fn truncate_visible(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        return s.to_string();
    }
    let mut result: String = chars.into_iter().take(max_len.saturating_sub(3)).collect();
    result.push_str("...");
    result
}
