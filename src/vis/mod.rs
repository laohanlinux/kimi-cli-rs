use axum::{
    routing::get,
    Json, Router,
};
use serde_json::json;

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
        .route("/healthz", get(health))
        .route("/traces", get(list_traces))
        .route("/metrics", get(metrics))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn list_traces() -> Json<serde_json::Value> {
    Json(json!({
        "traces": []
    }))
}

async fn metrics() -> Json<serde_json::Value> {
    Json(json!({
        "sessions_total": 0,
        "background_tasks_running": 0
    }))
}
