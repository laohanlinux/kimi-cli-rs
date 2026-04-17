use once_cell::sync::Lazy;

/// Application name displayed to users.
pub const NAME: &str = "Kimi Code CLI";

/// Application version loaded from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User-Agent header sent with HTTP requests.
pub static USER_AGENT: Lazy<String> = Lazy::new(|| format!("{NAME}/{VERSION}"));
