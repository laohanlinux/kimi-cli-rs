//! String helpers (ported from Python `utils/string.py`).

use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;

static NEWLINE_RUNS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\r\n]+").expect("newline regex"));

/// Shortens text to at most `width` **characters**, normalizing whitespace and preferring a word
/// boundary near the cut (Python `shorten`).
#[must_use]
pub fn shorten(text: &str, width: usize) -> String {
    shorten_with_placeholder(text, width, "…")
}

/// Same as [`shorten`] with a custom placeholder (Python `shorten(..., placeholder=...)`).
#[must_use]
pub fn shorten_with_placeholder(text: &str, width: usize, placeholder: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    if chars.len() <= width {
        return collapsed;
    }
    let ph_len = placeholder.chars().count();
    let cut = width.saturating_sub(ph_len);
    if cut == 0 {
        return chars.into_iter().take(width).collect();
    }
    let search_end = (cut + 1).min(chars.len());
    let mut last_space: Option<usize> = None;
    for i in 0..search_end {
        if chars[i] == ' ' {
            last_space = Some(i);
        }
    }
    let mut cut_at = cut;
    if let Some(sp) = last_space {
        if sp > 0 {
            cut_at = sp;
        }
    }
    let mut prefix: String = chars.iter().take(cut_at).collect();
    while prefix.ends_with(' ') {
        prefix.pop();
    }
    prefix + placeholder
}

/// Shortens by inserting `...` in the middle (Python `shorten_middle`).
#[must_use]
pub fn shorten_middle(text: &str, max_len: usize) -> String {
    shorten_middle_opts(text, max_len, true)
}

/// [`shorten_middle`] with optional newline → space normalization.
#[must_use]
pub fn shorten_middle_opts(text: &str, max_len: usize, remove_newline: bool) -> String {
    let t = if remove_newline {
        NEWLINE_RUNS.replace_all(text, " ").to_string()
    } else {
        text.to_string()
    };
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= max_len {
        return t;
    }
    let half = max_len / 2;
    let left: String = chars.iter().take(half).collect();
    let right: String = chars
        .iter()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{left}...{right}")
}

/// Lowercase ASCII random string (Python `random_string`, default length 8).
#[must_use]
pub fn random_string(length: usize) -> String {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..LETTERS.len());
            char::from(LETTERS[idx])
        })
        .collect()
}

/// Counts Unicode scalar values in a string (Python `len` on str).
#[must_use]
pub fn visible_len(s: &str) -> usize {
    s.chars().count()
}

/// Truncates to `max_len` characters with a trailing `...` (hard cut — no word boundary).
#[must_use]
pub fn truncate_visible(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        return s.to_string();
    }
    let mut result: String = chars.into_iter().take(max_len.saturating_sub(3)).collect();
    result.push_str("...");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_word_boundary() {
        let s = "hello world this is a long phrase";
        let out = shorten(s, 20);
        assert!(out.chars().count() <= 20, "{out:?}");
        assert!(out.contains('…'));
    }

    #[test]
    fn shorten_middle_long() {
        let s = "a".repeat(100);
        let out = shorten_middle(&s, 10);
        assert!(out.contains("..."));
        assert_eq!(out.len(), 13);
    }

    #[test]
    fn shorten_middle_short() {
        assert_eq!(shorten_middle("hi", 10), "hi");
    }

    #[test]
    fn random_string_length() {
        let s = random_string(16);
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_lowercase()));
    }
}
