#![recursion_limit = "512"]

/// Main entrypoint for the Kimi CLI.
#[tokio::main]
#[tracing::instrument(level = "info")]
async fn main() {
    if let Err(e) = _main().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn _main() -> Result<(), kimi_cli_rs::error::KimiCliError> {
    let args = kimi_cli_rs::cli::parse();

    // Initialize tracing subscriber.
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    if args.debug {
        tracing::info!("Debug logging enabled");
    }

    kimi_cli_rs::cli::run(&args).await
}
