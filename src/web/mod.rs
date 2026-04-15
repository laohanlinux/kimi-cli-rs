pub mod api;
pub mod runner;
pub mod store;


/// Axum web server for the agent.
#[derive(Debug, Clone, Default)]
pub struct WebServer {
    pub port: u16,
}

impl WebServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(&self) -> crate::error::Result<()> {
        let state = api::WebAppState::default();
        let app = api::router()
            .with_state(state.clone())
            .route("/", axum::routing::get(|| async { "Kimi CLI Web Server" }));

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port))
            .await
            .map_err(|e| crate::error::KimiCliError::Io(e))?;
        tracing::info!("Web server listening on port {}", self.port);
        axum::serve(listener, app)
            .await
            .map_err(|e| crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }
}
