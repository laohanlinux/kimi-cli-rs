pub mod agent;
pub mod approval;
pub mod compaction;
pub mod context;
pub mod denwa_renji;
pub mod dynamic_injection;
pub mod dynamic_injections;
pub mod kimisoul;
pub mod message;
pub mod slash;
pub mod toolset;

/// Reasons a turn may stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStopReason {
    NoToolCalls,
    ToolRejected,
    MaxStepsReached,
    Cancelled,
}

/// Outcome of a single turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub stop_reason: TurnStopReason,
    pub final_message: Option<crate::soul::message::Message>,
    pub step_count: usize,
}

/// Soul status snapshot for UI display.
#[derive(Debug, Clone, Copy)]
pub struct StatusSnapshot {
    pub context_usage: f64,
    pub yolo_enabled: bool,
    pub plan_mode: bool,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
}

/// Formats a token count as a compact string (e.g., 28.5k, 1.2m).
pub fn format_token_count(n: usize) -> String {
    if n >= 1_000_000 {
        let value = n as f64 / 1_000_000.0;
        let compact = format!("{:.1}", value).trim_end_matches('0').trim_end_matches('.').to_string();
        format!("{compact}m")
    } else if n >= 1_000 {
        let value = n as f64 / 1_000.0;
        let compact = format!("{:.1}", value).trim_end_matches('0').trim_end_matches('.').to_string();
        format!("{compact}k")
    } else {
        n.to_string()
    }
}

/// Formats context status for the status bar.
pub fn format_context_status(context_usage: f64, context_tokens: usize, max_context_tokens: usize) -> String {
    let bounded = context_usage.clamp(0.0, 1.0);
    let pct = format!("{:.1}", bounded * 100.0);
    if max_context_tokens > 0 {
        let used = format_token_count(context_tokens);
        let total = format_token_count(max_context_tokens);
        format!("context: {pct}% ({used}/{total})")
    } else {
        format!("context: {pct}%")
    }
}

/// Orchestrates a soul run with its UI loop and notification pump.
#[tracing::instrument(level = "info", skip_all)]
pub async fn run_soul(
    soul: &mut crate::soul::kimisoul::KimiSoul,
    user_input: Vec<crate::soul::message::ContentPart>,
    ui_loop_fn: impl FnOnce(crate::wire::Wire) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    mut cancel_event: tokio::sync::watch::Receiver<bool>,
    _runtime: &crate::soul::agent::Runtime,
) -> crate::error::Result<TurnOutcome> {
    let wire = crate::wire::Wire::default();
    let ui_task = tokio::spawn(ui_loop_fn(wire.clone()));

    // Approval response pump: forwards wire responses into the soul.
    let approval_tx = soul.approval_tx();
    let mut response_rx = wire.response_rx();
    let approval_pump = tokio::spawn(async move {
        if let Some(ref mut rx) = response_rx {
            while let Some(msg) = rx.recv().await {
                let _ = approval_tx.send(msg);
            }
        }
    });

    // Simple cancellation check before starting.
    if *cancel_event.borrow() {
        drop(approval_pump);
        let _ = ui_task.await;
        return Ok(TurnOutcome {
            stop_reason: TurnStopReason::Cancelled,
            final_message: None,
            step_count: 0,
        });
    }

    let result = soul.run(user_input).await;

    // Graceful shutdown: give UI a moment to process final messages.
    wire.soul_side().send_merged(crate::wire::types::WireMessage::TurnEnd {
        stop_reason: "complete".into(),
    });

    drop(approval_pump);
    let _ = ui_task.await;

    match result {
        Ok(outcome) => Ok(outcome),
        Err(e) => {
            tracing::error!("soul run failed: {}", e);
            Ok(TurnOutcome {
                stop_reason: TurnStopReason::NoToolCalls,
                final_message: None,
                step_count: 0,
            })
        }
    }
}
