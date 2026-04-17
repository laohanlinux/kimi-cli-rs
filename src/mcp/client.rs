use std::collections::HashMap;

use rmcp::{ServiceExt, transport::TokioChildProcess};

/// MCP client connection handle.
pub struct McpConnection {
    pub peer: rmcp::service::Peer<rmcp::service::RoleClient>,
    cancel: rmcp::service::RunningServiceCancellationToken,
}

impl std::fmt::Debug for McpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConnection")
            .field("peer", &self.peer)
            .finish()
    }
}

impl McpConnection {
    /// Gracefully cancels the background service.
    pub fn cancel(self) {
        self.cancel.cancel();
    }
}

/// Connects to an MCP server over stdio.
#[tracing::instrument(level = "debug", skip(env))]
pub async fn connect_stdio(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> crate::error::Result<McpConnection> {
    let mut full_env = crate::utils::subprocess_env::get_clean_env();
    full_env.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args);
    crate::utils::subprocess_env::apply_to_tokio(&mut cmd, full_env);

    let transport = TokioChildProcess::new(cmd)
        .map_err(|e| crate::error::KimiCliError::McpRuntime(format!("stdio transport: {e}")))?;

    let running = ()
        .serve(transport)
        .await
        .map_err(|e| crate::error::KimiCliError::McpRuntime(format!("MCP init failed: {e}")))?;

    let peer = running.peer().clone();
    let cancel = running.cancellation_token();

    // Keep the running service alive in the background.
    tokio::spawn(async move {
        let _ = running.waiting().await;
    });

    Ok(McpConnection { peer, cancel })
}

/// Connects to an MCP server over HTTP.
#[tracing::instrument(level = "debug", skip(headers))]
pub async fn connect_http(
    url: &str,
    headers: &HashMap<String, String>,
) -> crate::error::Result<McpConnection> {
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    let mut config = StreamableHttpClientTransportConfig::with_uri(url);
    for (k, v) in headers {
        if let (Ok(name), Ok(value)) = (
            k.parse::<http::HeaderName>(),
            v.parse::<http::HeaderValue>(),
        ) {
            config.custom_headers.insert(name, value);
        }
    }

    let transport = rmcp::transport::StreamableHttpClientTransport::from_config(config);

    let running = ()
        .serve(transport)
        .await
        .map_err(|e| crate::error::KimiCliError::McpRuntime(format!("MCP init failed: {e}")))?;

    let peer = running.peer().clone();
    let cancel = running.cancellation_token();

    tokio::spawn(async move {
        let _ = running.waiting().await;
    });

    Ok(McpConnection { peer, cancel })
}
