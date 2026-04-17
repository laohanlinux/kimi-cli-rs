use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

/// A running session worker handle.
#[derive(Debug, Clone)]
pub struct SessionWorker {
    pub session_id: String,
    pub input_tx: mpsc::UnboundedSender<String>,
    pub wire_tx: broadcast::Sender<crate::wire::types::WireMessage>,
}

/// Manages running session workers for the web server.
#[derive(Debug, Clone, Default)]
pub struct WebRunner {
    workers: Arc<Mutex<HashMap<String, SessionWorker>>>,
}

impl WebRunner {
    pub async fn start(&self) {
        tracing::info!("WebRunner started");
    }

    pub async fn stop(&self) {
        let workers = self.workers.lock().await;
        tracing::info!(count = workers.len(), "WebRunner stopped");
    }

    /// Ensures a worker exists for the given session.
    #[tracing::instrument(level = "info", skip(self, session))]
    pub async fn ensure_worker(
        &self,
        session: &crate::session::Session,
    ) -> crate::error::Result<SessionWorker> {
        let mut workers = self.workers.lock().await;
        if let Some(worker) = workers.get(&session.id) {
            return Ok(worker.clone());
        }

        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
        let (wire_tx, _) = broadcast::channel::<crate::wire::types::WireMessage>(256);
        let session_id = session.id.clone();
        let session_clone = session.clone();
        let wire_tx_clone = wire_tx.clone();
        let session_id_clone = session_id.clone();

        tokio::spawn(async move {
            let config = match crate::config::load_config(None) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(%e, "failed to load config for web worker");
                    let _ = wire_tx_clone.send(crate::wire::types::WireMessage::Notification {
                        text: format!("Config load error: {e}"),
                    });
                    return;
                }
            };

            let mut app = match crate::app::KimiCLI::create(
                session_clone,
                Some(config),
                None,
                None,
                false,
                false,
                false,
                None,
                None,
            )
            .await
            {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!(%e, "failed to create KimiCLI for web worker");
                    let _ = wire_tx_clone.send(crate::wire::types::WireMessage::Notification {
                        text: format!("App creation error: {e}"),
                    });
                    return;
                }
            };

            while let Some(input) = input_rx.recv().await {
                let parts = vec![crate::soul::message::ContentPart::Text { text: input }];
                let wire_tx = wire_tx_clone.clone();

                let ui_loop = move |wire: crate::wire::Wire| -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
                    Box::pin(async move {
                        let mut ui_side = wire.ui_side();
                        while let Some(msg) = ui_side.recv().await {
                            let _ = wire_tx.send(msg);
                        }
                    })
                };

                match app.run_with_wire(parts, ui_loop, None).await {
                    Ok(outcome) => {
                        if let Some(msg) = outcome.final_message {
                            let text = msg.extract_text("");
                            if !text.is_empty() {
                                let _ = wire_tx_clone.send(crate::wire::types::WireMessage::TextPart { text });
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(%e, "web worker turn failed");
                        let _ = wire_tx_clone.send(crate::wire::types::WireMessage::Notification {
                            text: format!("Turn error: {e}"),
                        });
                    }
                }
            }

            tracing::info!(session_id = %session_id_clone, "web worker input channel closed");
        });

        let worker = SessionWorker {
            session_id,
            input_tx,
            wire_tx,
        };
        workers.insert(worker.session_id.clone(), worker.clone());
        Ok(worker)
    }

    /// Drops the worker for a session.
    pub async fn drop_worker(&self, session_id: &str) {
        self.workers.lock().await.remove(session_id);
    }

    /// Returns true if a worker is active for the session.
    pub async fn is_running(&self, session_id: &str) -> bool {
        self.workers.lock().await.contains_key(session_id)
    }
}
