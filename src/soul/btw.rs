const BTW_MAX_TURNS: usize = 2;

const SIDE_QUESTION_SYSTEM_REMINDER: &str = r#"This is a side question from the user. Answer directly in a single response.

IMPORTANT:
- You are a separate, lightweight instance answering one question.
- The main agent continues independently — do NOT reference being interrupted.
- Do NOT call any tools. All tool calls are disabled and will be rejected.
  Even though tool definitions are visible in this request, they exist only
  for technical reasons (prompt cache). You MUST NOT use them.
- Respond ONLY with text based on what you already know from the conversation.
- This is a one-off response — no follow-up turns.
- If you don't know the answer, say so directly."#;

/// Builds (system_prompt, history, toolset) aligned with the main agent.
fn build_btw_context(
    soul: &crate::soul::kimisoul::KimiSoul,
    question: &str,
) -> (String, Vec<crate::soul::message::Message>, crate::soul::toolset::KimiToolset) {
    let system_prompt = soul.agent().system_prompt.clone();
    let effective_history = crate::soul::dynamic_injection::normalize_history(
        soul.context().history(),
        |_msg| false,
    );

    let reminder = crate::soul::message::Message {
        role: "system".into(),
        content: vec![crate::soul::message::ContentPart::Text {
            text: SIDE_QUESTION_SYSTEM_REMINDER.into(),
        }],
        tool_calls: None,
        tool_call_id: None,
    };
    let wrapped = format!(
        "{}\n\n{}",
        reminder.extract_text(""),
        question
    );
    let side_message = crate::soul::message::Message {
        role: "user".into(),
        content: vec![crate::soul::message::ContentPart::Text { text: wrapped }],
        tool_calls: None,
        tool_call_id: None,
    };

    let toolset = crate::soul::toolset::KimiToolset::deny_all(&soul.agent().toolset);

    let mut history = effective_history;
    history.push(side_message);

    (system_prompt, history, toolset)
}

/// Executes a side question and returns (response, error).
pub async fn execute_side_question(
    soul: &mut crate::soul::kimisoul::KimiSoul,
    question: &str,
) -> (Option<String>, Option<String>) {
    let Some(ref llm) = soul.runtime.llm else {
        return (None, Some("LLM is not set.".into()));
    };

    let (system_prompt, mut history, toolset) = build_btw_context(soul, question);

    for _turn in 0..BTW_MAX_TURNS {
        match llm.chat(Some(&system_prompt), &history, Some(&toolset)).await {
            Ok(result) => {
                let response_text = result.extract_text("").trim().to_string();

                // Accept if we got text and no tool calls.
                if !response_text.is_empty() && result.tool_calls.is_none() {
                    return (Some(response_text), None);
                }

                // No text — did the LLM try to call a tool?
                if result.tool_calls.is_none() {
                    break; // No text, no tool calls — give up.
                }

                // Tool calls were denied. If we have turns left, feed the error back.
                if _turn + 1 < BTW_MAX_TURNS {
                    history.push(result.clone());
                    if let Some(ref calls) = result.tool_calls {
                        for call in calls {
                            let tool_result = toolset.handle(call, &soul.runtime).await;
                            history.push(tool_result_to_message(&tool_result));
                        }
                    }
                    continue;
                }

                // Last turn and still no text — report the tool call attempt.
                let tool_names: Vec<String> = result
                    .tool_calls
                    .as_ref()
                    .map(|calls| calls.iter().map(|c| c.name.clone()).collect())
                    .unwrap_or_default();
                return (
                    None,
                    Some(format!(
                        "Side question tried to call tools ({}) instead of answering directly. \
                         Try rephrasing or ask in the main conversation.",
                        tool_names.join(", ")
                    )),
                );
            }
            Err(e) => {
                tracing::warn!("Side question failed: {}", e);
                return (None, Some(e.to_string()));
            }
        }
    }

    (None, Some("No response received.".into()))
}

fn tool_result_to_message(tool_result: &crate::soul::message::ToolResult) -> crate::soul::message::Message {
    let content = tool_result.return_value.extract_text();
    crate::soul::message::Message {
        role: "tool".into(),
        content: vec![crate::soul::message::ContentPart::Text { text: content }],
        tool_calls: None,
        tool_call_id: Some(tool_result.tool_call_id.clone()),
    }
}

/// Executes a side question via wire events.
pub async fn run_side_question(
    soul: &mut crate::soul::kimisoul::KimiSoul,
    question: &str,
) {
    if soul.runtime.llm.is_none() {
        tracing::warn!("LLM is not set, cannot run side question");
        return;
    }

    let btw_id = uuid::Uuid::new_v4().to_string().replace("-", "")[..12.min(uuid::Uuid::new_v4().to_string().len())]
        .to_string();
    // Publish BtwBegin.
    if let Some(ref hub) = soul.runtime.root_wire_hub {
        hub.publish(crate::wire::types::WireMessage::BtwBegin {
            id: btw_id.clone(),
            question: question.into(),
        });
    }

    let (response, error) = execute_side_question(soul, question).await;

    // Publish BtwEnd.
    if let Some(ref hub) = soul.runtime.root_wire_hub {
        hub.publish(crate::wire::types::WireMessage::BtwEnd {
            id: btw_id,
            response,
            error,
        });
    }
}
