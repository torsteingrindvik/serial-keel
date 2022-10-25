use tokio::sync::RwLock;
use tracing::metadata::LevelFilter;
use tracing_subscriber::{prelude::*, EnvFilter};

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

        tracing_subscriber::registry()
            .with(tracing_tracy::TracyLayer::new().with_filter(LevelFilter::TRACE))
            .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::new(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
            )))
            .init();

        *initialized = true;
    }
}
