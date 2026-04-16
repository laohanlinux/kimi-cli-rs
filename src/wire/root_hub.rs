use tokio::sync::broadcast;

/// Session-level broadcast hub for out-of-turn messages.
#[derive(Debug, Clone)]
pub struct RootWireHub {
    tx: broadcast::Sender<crate::wire::types::WireMessage>,
}

impl RootWireHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<crate::wire::types::WireMessage> {
        self.tx.subscribe()
    }

    pub fn publish(&self, msg: crate::wire::types::WireMessage) {
        let _ = self.tx.send(msg);
    }

    pub fn publish_nowait(&self, msg: crate::wire::types::WireMessage) {
        let _ = self.tx.send(msg);
    }
}

impl Default for RootWireHub {
    fn default() -> Self {
        Self::new()
    }
}
