#![recursion_limit = "512"]

use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// Main entrypoint for the Kimi CLI.
#[tokio::main]
#[tracing::instrument(level = "info", skip_all)]
async fn main() -> Result<(), kimi_cli_rs::error::KimiCliError> {
    kimi_cli_rs::utils::proxy::normalize_proxy_env();
    let args = kimi_cli_rs::cli::parse();
    init_logging(args.debug);
    if args.debug {
        tracing::info!("Debug logging enabled (verbose file log; see RUST_LOG / RUST_LOG_TTY)");
    }
    kimi_cli_rs::cli::run(&args).await
}

/// Initializes tracing: by default almost everything goes to the log file; the terminal only
/// shows high-severity lines so the TUI is not interleaved with noise.
///
/// - `RUST_LOG`: filter for the log file (default: `info`, or `debug` when `debug` is true).
/// - `RUST_LOG_TTY`: filter for stderr (default: `error`, or `warn` when `debug` is true).
fn init_logging(debug: bool) {
    let file_directive = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        if debug {
            "debug".to_owned()
        } else {
            "info".to_owned()
        }
    });
    let file_filter = tracing_subscriber::EnvFilter::try_new(&file_directive)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let tty_directive = std::env::var("RUST_LOG_TTY").unwrap_or_else(|_| {
        if debug {
            "warn".to_owned()
        } else {
            "error".to_owned()
        }
    });
    let stderr_filter = tracing_subscriber::EnvFilter::try_new(&tty_directive)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error"));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(stderr_filter);

    let file_layer = if let Ok(share_dir) = kimi_cli_rs::share::get_share_dir() {
        let logs_dir = share_dir.join("logs");
        if std::fs::create_dir_all(&logs_dir).is_ok() {
            let log_path = logs_dir.join("kimi-cli-rs.log");
            if let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                Some(
                    tracing_subscriber::fmt::layer()
                        .with_writer(move || file.try_clone().expect("log file clone failed"))
                        .with_ansi(false)
                        .with_filter(file_filter),
                )
            } else {
                eprintln!("Warning: failed to open log file at {}", log_path.display());
                None
            }
        } else {
            eprintln!(
                "Warning: failed to create logs directory at {}",
                logs_dir.display()
            );
            None
        }
    } else {
        eprintln!("Warning: failed to determine share directory for logs");
        None
    };

    let subscriber = tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer);

    if let Err(err) = subscriber.try_init() {
        eprintln!("Warning: failed to set global tracing subscriber: {err}");
    }
}
