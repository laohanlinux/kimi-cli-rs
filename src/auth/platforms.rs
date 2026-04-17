/// Platform-specific OAuth integrations.
#[derive(Debug, Clone, Default)]
pub struct PlatformIntegrations;

impl PlatformIntegrations {
    /// Refreshes the access token for the given platform if supported.
    #[tracing::instrument(level = "debug")]
    pub async fn refresh_token(&self, _platform: &str, _refresh_token: &str) -> Option<String> {
        // Platform-specific refresh flows (e.g., Google, GitHub) are not yet implemented.
        None
    }
}
