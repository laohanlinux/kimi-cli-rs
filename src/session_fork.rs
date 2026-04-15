use std::io::BufRead;

/// Forks a session, optionally truncating at a given turn index.
///
/// A turn index of `Some(n)` means: keep all wire records for turns 0..=n.
/// The context file is truncated by counting user messages (each turn starts
/// with exactly one user message) and dropping everything from the next turn
/// onward.
#[tracing::instrument(level = "debug", skip(source))]
pub async fn fork(
    source: &crate::session::Session,
    turn_index: Option<usize>,
) -> crate::error::Result<crate::session::Session> {
    let new_session = crate::session::create(source.work_dir.clone(), None, None).await?;

    // Copy context file, optionally truncating at the given turn index.
    if source.context_file.exists() {
        let content = tokio::fs::read_to_string(&source.context_file).await?;
        let truncated = if let Some(limit) = turn_index {
            truncate_context_at_turn(&content, limit)
        } else {
            content
        };
        tokio::fs::write(&new_session.context_file, truncated).await?;
    }

    // Copy wire file, optionally truncating at the given turn index.
    if source.wire_file.path.exists() {
        let mut turn_counter: usize = 0;
        let mut kept_lines = Vec::new();
        let file = std::fs::File::open(&source.wire_file.path)?;
        let reader = std::io::BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(limit) = turn_index {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(t) = value.get("type").and_then(|v| v.as_str()) {
                        if t == "turn_begin" {
                            if turn_counter > limit {
                                break;
                            }
                            turn_counter += 1;
                        }
                    }
                }
            }
            kept_lines.push(line);
        }
        let wire_text = kept_lines.join("\n");
        if !wire_text.is_empty() {
            tokio::fs::write(&new_session.wire_file.path, wire_text + "\n").await?;
        }
    }

    // Copy session state, but reset plan/fork-specific fields.
    let mut new_state = crate::session_state::load_session_state(&source.dir());
    new_state.custom_title = Some(format!("Fork of {}", source.title));
    new_state.title_generated = true;
    new_state.plan_mode = false;
    new_state.plan_session_id = None;
    new_state.plan_slug = None;
    crate::session_state::save_session_state(&new_state, &new_session.dir())?;

    Ok(new_session)
}

/// Truncates a JSONL context dump after the given turn limit.
///
/// Turns are counted by `role == "user"` messages. All records (system
/// prompts, usage, checkpoints, assistant/tool messages) belonging to turns
/// `0..=limit` are preserved.
fn truncate_context_at_turn(content: &str, limit: usize) -> String {
    let mut user_turn_count: usize = 0;
    let mut kept = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_user_message = serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|v| v.get("role").and_then(|r| r.as_str()).map(|r| r == "user"))
            == Some(true);
        if is_user_message {
            if user_turn_count > limit {
                break;
            }
            user_turn_count += 1;
        }
        kept.push(trimmed);
    }
    if kept.is_empty() {
        String::new()
    } else {
        kept.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_context_at_turn;

    #[test]
    fn truncate_context_keeps_all_when_under_limit() {
        let ctx = r#"{"role":"_system_prompt","content":"hi"}
{"role":"user","content":[{"type":"text","text":"hello"}]}
{"role":"assistant","content":[{"type":"text","text":"world"}]}
"#;
        assert_eq!(truncate_context_at_turn(ctx, 5), ctx);
    }

    #[test]
    fn truncate_context_drops_after_limit() {
        let ctx = r#"{"role":"user","content":[{"type":"text","text":"t1"}]}
{"role":"assistant","content":[{"type":"text","text":"a1"}]}
{"role":"user","content":[{"type":"text","text":"t2"}]}
{"role":"assistant","content":[{"type":"text","text":"a2"}]}
"#;
        let expected = r#"{"role":"user","content":[{"type":"text","text":"t1"}]}
{"role":"assistant","content":[{"type":"text","text":"a1"}]}
"#;
        assert_eq!(truncate_context_at_turn(ctx, 0), expected);
    }

    #[test]
    fn truncate_context_preserves_metadata() {
        let ctx = r#"{"role":"_system_prompt","content":"sys"}
{"role":"_checkpoint","id":0}
{"role":"user","content":[{"type":"text","text":"u"}]}
{"role":"_usage","token_count":4}
{"role":"assistant","content":[{"type":"text","text":"a"}]}
{"role":"user","content":[{"type":"text","text":"next"}]}
"#;
        let expected = r#"{"role":"_system_prompt","content":"sys"}
{"role":"_checkpoint","id":0}
{"role":"user","content":[{"type":"text","text":"u"}]}
{"role":"_usage","token_count":4}
{"role":"assistant","content":[{"type":"text","text":"a"}]}
"#;
        assert_eq!(truncate_context_at_turn(ctx, 0), expected);
    }
}
