use crate::skill::flow::{Flow, FlowError};

/// Parses a D2 flowchart into a Flow.
#[tracing::instrument(level = "debug")]
pub fn parse_d2_flowchart(_input: &str) -> Result<Flow, FlowError> {
    // TODO: implement D2 flowchart parsing
    tracing::info!("D2 flowchart parsing not yet implemented");
    Ok(Flow::default())
}
