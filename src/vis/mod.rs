use axum::{
    routing::get,
    Router,
};

pub mod api;

/// Visualization server for tracing and diagnostics.
#[derive(Debug, Clone, Default)]
pub struct VisServer {
    pub port: u16,
}

impl VisServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(&self) -> crate::error::Result<()> {
        let app = router();

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", self.port))
            .await
            .map_err(|e| crate::error::KimiCliError::Io(e))?;
        tracing::info!("Vis server listening on port {}", self.port);
        axum::serve(listener, app)
            .await
            .map_err(|e| crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }
}

/// Builds the Vis API router.
pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }))
        .route("/api/sessions", get(api::list_sessions))
        .route("/api/sessions/:id/wire", get(api::get_wire_events))
        .route("/traces", get(api::list_traces))
        .route("/metrics", get(api::metrics))
        .fallback(api::spa_fallback)
}
