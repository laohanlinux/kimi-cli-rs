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
        if let Ok(cwd) = std::env::current_dir() {
            state.store.write().await.load_sessions(&cwd).await;
        }
        let app = api::router()
            .with_state(state.clone())
            .route("/", axum::routing::get(|| async { "Kimi CLI Web Server" }));

        const MAX_PORT_ATTEMPTS: u32 = 10;
        let listener =
            crate::utils::server::bind_tcp_listener("127.0.0.1", self.port, MAX_PORT_ATTEMPTS)
                .await
                .map_err(crate::error::KimiCliError::Io)?;
        let addr = listener
            .local_addr()
            .map_err(crate::error::KimiCliError::Io)?;
        tracing::info!("{}", crate::utils::server::format_url_for_addr(addr));
        axum::serve(listener, app).await.map_err(|e| {
            crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        Ok(())
    }
}
