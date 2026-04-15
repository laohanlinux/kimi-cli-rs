use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Bridges the internal Wire to a JSON-RPC over stdio client.
pub struct WireServer {
    // Placeholder: full implementation requires soul integration
}

impl WireServer {
    pub fn new() -> Self {
        Self {}
    }

    #[tracing::instrument(level = "info", skip(self))]
    pub async fn serve(self) -> crate::error::Result<()> {
        let stdin = BufReader::new(io::stdin());
        let mut stdout = io::stdout();
        let mut lines = stdin.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(bytes = line.len(), "jsonrpc read line");
            let _request: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(%e, "invalid jsonrpc line");
                    continue;
                }
            };
            let response = serde_json::json!({"jsonrpc": "2.0", "result": null});
            let out = format!("{}\n", serde_json::to_string(&response)?);
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
        Ok(())
    }
}
