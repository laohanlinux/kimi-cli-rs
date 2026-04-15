pub mod file;
pub mod root_hub;
pub mod server;
pub mod types;

use tokio::sync::{broadcast, mpsc};

/// Single-producer, multi-consumer wire channel.
#[derive(Debug)]
pub struct Wire {
    raw_tx: broadcast::Sender<types::WireMessage>,
    merged_tx: broadcast::Sender<types::WireMessage>,
    response_tx: mpsc::Sender<types::WireMessage>,
    response_rx: std::sync::Mutex<Option<mpsc::Receiver<types::WireMessage>>>,
}

impl Wire {
    /// Creates a new wire channel.
    pub fn new() -> Self {
        let (raw_tx, _) = broadcast::channel::<types::WireMessage>(1024);
        let (merged_tx, _) = broadcast::channel::<types::WireMessage>(1024);
        let (response_tx, response_rx) = mpsc::channel::<types::WireMessage>(1024);
        Self {
            raw_tx,
            merged_tx,
            response_tx,
            response_rx: std::sync::Mutex::new(Some(response_rx)),
        }
    }

    /// Takes the response receiver (only valid once).
    pub fn response_rx(&self) -> Option<mpsc::Receiver<types::WireMessage>> {
        self.response_rx.lock().ok()?.take()
    }

    /// Returns the soul-side sender.
    pub fn soul_side(&self) -> WireSoulSide {
        WireSoulSide {
            raw_tx: self.raw_tx.clone(),
            merged_tx: self.merged_tx.clone(),
        }
    }

    /// Returns the UI-side receiver.
    pub fn ui_side(&self) -> WireUISide {
        WireUISide {
            raw_rx: self.raw_tx.subscribe(),
            merged_rx: self.merged_tx.subscribe(),
            response_tx: self.response_tx.clone(),
        }
    }
}

impl Default for Wire {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Wire {
    fn clone(&self) -> Self {
        Self {
            raw_tx: self.raw_tx.clone(),
            merged_tx: self.merged_tx.clone(),
            response_tx: self.response_tx.clone(),
            response_rx: std::sync::Mutex::new(None),
        }
    }
}

/// Producer side of the wire channel.
pub struct WireSoulSide {
    raw_tx: broadcast::Sender<types::WireMessage>,
    merged_tx: broadcast::Sender<types::WireMessage>,
}

impl WireSoulSide {
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn send(&self, msg: types::WireMessage) {
        let _ = self.raw_tx.send(msg.clone());
        let _ = self.merged_tx.send(msg);
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn send_merged(&self, msg: types::WireMessage) {
        let _ = self.merged_tx.send(msg);
    }
}

/// Consumer side of the wire channel.
pub struct WireUISide {
    raw_rx: broadcast::Receiver<types::WireMessage>,
    merged_rx: broadcast::Receiver<types::WireMessage>,
    response_tx: mpsc::Sender<types::WireMessage>,
}

impl WireUISide {
    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn recv(&mut self) -> Option<types::WireMessage> {
        self.merged_rx.recv().await.ok()
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub async fn send_response(&self, msg: types::WireMessage) {
        let _ = self.response_tx.send(msg).await;
    }
}
