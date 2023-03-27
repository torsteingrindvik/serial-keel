use std::path::PathBuf;

use tokio::sync::RwLock;
use tracing::Level;
use tracing::{debug, info, metadata::LevelFilter, trace};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::prelude::*;

fn do_init(stdout_level: Level, file_level: Option<(Level, PathBuf)>) {
    let mut message = String::from("Logging with:");

    // stdout
    message += " stdout";

    #[cfg(feature = "use-tracy")]
    let registry = {
        message += ", tracy";

        let filter = LevelFilter::DEBUG;
        let layer = tracing_tracy::TracyLayer::new();
        registry.with(layer.with_filter(filter))
    };

    #[cfg(feature = "use-jaeger")]
    let registry = {
        message += ", jaeger";

        opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

        let filter = LevelFilter::INFO;
        let layer = tracing_opentelemetry::layer();
        let tracer = opentelemetry_jaeger::new_agent_pipeline()
            // .with_endpoint("localhost:6831") // TODO
            // .with_service_name("hello") // TODO
            .install_batch(opentelemetry::runtime::Tokio)
            .expect("Installing Jaeger should work");

        registry.with(layer.with_tracer(tracer).with_filter(filter))
    };

    #[cfg(feature = "use-zipkin")]
    let registry = {
        message += ", zipkin";

        opentelemetry::global::set_text_map_propagator(opentelemetry_zipkin::Propagator::new());

        let filter = LevelFilter::INFO;
        let layer = tracing_opentelemetry::layer();
        let tracer = opentelemetry_zipkin::new_pipeline()
            // .with_endpoint("localhost:6831") // TODO
            // .with_service_name("hello") // TODO
            .install_batch(opentelemetry::runtime::Tokio)
            .expect("Installing Zipkin should work");

        registry.with(layer.with_tracer(tracer).with_filter(filter))
    };

    let stdout_layer =
        tracing_subscriber::fmt::layer().with_filter(LevelFilter::from(stdout_level));

    let registry = tracing_subscriber::registry().with(stdout_layer);

    let maybe_file_layer = if let Some((level, output_dir)) = file_level {
        message += &format!(", file (in dir {output_dir:?})");

        let file_appender = RollingFileAppender::new(Rotation::DAILY, output_dir, "sk.log");

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_filter(LevelFilter::from(level));
        Some(file_layer)
    } else {
        None
    };

    registry.with(maybe_file_layer).init();

    #[cfg(any(feature = "use-jaeger", feature = "use-zipkin"))]
    {
        use std::time::Duration;

        use tracing::{debug_span, Instrument};

        tokio::spawn(
            async move {
                let mut count = 0;
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    debug!(%count, "Alive");
                    count += 1;
                }
            }
            .instrument(debug_span!("otel-pulse")),
        );
    }

    debug!(message);
}

/// Initialize tracing.
///
/// Will only initialize once, so tests may call this.
pub async fn init(stdout_level: Level, file_logging: Option<(Level, PathBuf)>) {
    static TRACING_IS_INITIALIZED: RwLock<bool> = RwLock::const_new(false);

    let initialized = { *TRACING_IS_INITIALIZED.read().await };

    if !initialized {
        let mut initialized = TRACING_IS_INITIALIZED.write().await;

        // To avoid race condition between the `.read()` and the
        // `.write()`.
        if *initialized {
            return;
        }

        do_init(stdout_level, file_logging);

        *initialized = true;
    }

    info!("Logging initialized");
}

/// Export any spans not exported yet.
pub fn shutdown() {
    trace!("Shutting down");
    #[cfg(any(feature = "use-jaeger", feature = "use-zipkin"))]
    opentelemetry::global::shutdown_tracer_provider();
}
