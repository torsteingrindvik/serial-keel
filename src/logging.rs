use tokio::sync::RwLock;
use tracing_subscriber::{prelude::*, EnvFilter};

#[cfg(not(feature = "tracy"))]
fn do_init() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        )))
        .init();
}

#[cfg(feature = "tracy")]
fn do_init() {
    use tracing::metadata::LevelFilter;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        )))
        .with(tracing_tracy::TracyLayer::new().with_filter(LevelFilter::DEBUG))
        .init();
}

/// Initialize tracing.
///
/// Will only initialize once, so tests may call this.
pub async fn init() {
    static TRACING_IS_INITIALIZED: RwLock<bool> = RwLock::const_new(false);

    let initialized = { *TRACING_IS_INITIALIZED.read().await };

    if !initialized {
        let mut initialized = TRACING_IS_INITIALIZED.write().await;

        // To avoid race condition between the `.read()` and the
        // `.write()`.
        if *initialized {
            return;
        }

        do_init();

        *initialized = true;
    }
}
