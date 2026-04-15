/// Web task runner.
#[derive(Debug, Clone, Default)]
pub struct WebRunner;

impl WebRunner {
    pub async fn start(&self) {
        tracing::info!("WebRunner started");
    }

    pub async fn stop(&self) {
        tracing::info!("WebRunner stopped");
    }
}
