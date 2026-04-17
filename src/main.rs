#![recursion_limit = "512"]

/// Main entrypoint for the Kimi CLI.
#[tokio::main]
#[tracing::instrument(level = "info")]
async fn main() -> Result<(), kimi_cli_rs::error::KimiCliError> {
    // Initialize tracing subscriber.
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    if let Err(err) = tracing::subscriber::set_global_default(subscriber) {
        // Don't panic during startup if another global subscriber was already installed.
        eprintln!("Warning: failed to set global tracing subscriber: {err}");
    }
    let args = kimi_cli_rs::cli::parse();
    if args.debug {
        tracing::info!("Debug logging enabled");
    }
    kimi_cli_rs::cli::run(&args).await
}
