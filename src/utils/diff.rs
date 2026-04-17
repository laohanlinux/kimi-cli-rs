/// Renders a unified diff between old and new text.
pub fn render_unified_diff(old_text: &str, new_text: &str, context: usize) -> String {
    similar::TextDiff::from_lines(old_text, new_text)
        .unified_diff()
        .context_radius(context)
        .to_string()
}
