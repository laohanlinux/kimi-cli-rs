use axum::{Router, routing::get};

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

        const MAX_PORT_ATTEMPTS: u32 = 10;
        let listener =
            crate::utils::server::bind_tcp_listener("127.0.0.1", self.port, MAX_PORT_ATTEMPTS)
                .await
                .map_err(crate::error::KimiCliError::Io)?;
        let addr = listener
            .local_addr()
            .map_err(crate::error::KimiCliError::Io)?;
        tracing::info!(
            "Vis server at {}",
            crate::utils::server::format_url_for_addr(addr)
        );
        axum::serve(listener, app).await.map_err(|e| {
            crate::error::KimiCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        Ok(())
    }
}

/// Builds the Vis API router.
pub fn router() -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/api/sessions", get(api::list_sessions))
        .route("/api/sessions/:id/wire", get(api::get_wire_events))
        .route("/traces", get(api::list_traces))
        .route("/metrics", get(api::metrics))
        .route("/api/statistics", get(api::statistics))
        .route("/api/system", get(api::system_info))
        .fallback(api::spa_fallback)
}
