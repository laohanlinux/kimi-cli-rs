use crate::skill::flow::{Flow, FlowError};

/// Parses a Mermaid flowchart into a Flow.
#[tracing::instrument(level = "debug")]
pub fn parse_mermaid_flowchart(_input: &str) -> Result<Flow, FlowError> {
    // TODO: implement Mermaid flowchart parsing
    tracing::info!("Mermaid flowchart parsing not yet implemented");
    Ok(Flow::default())
}
