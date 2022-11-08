use tokio::sync::RwLock;
use tracing::{info, metadata::LevelFilter};
use tracing_subscriber::{layer::Filter, prelude::*, EnvFilter};

fn do_init() {
    let registry = tracing_subscriber::registry();

    let mut message = String::from("Logging with:");

    // stdout
    let filter: Box<dyn Filter<_> + Send + Sync> = match EnvFilter::try_from_default_env() {
        Ok(env) => Box::new(env),
        Err(_) => Box::new(LevelFilter::INFO),
    };
    message += " stdout";

    // .unwrap_or_else(|_| LevelFilter::INFO),
    // let layer = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);
    let layer = tracing_subscriber::fmt::layer();
    let registry = registry.with(layer.with_filter(filter));

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

    registry.init();

    #[cfg(any(feature = "use-jaeger", feature = "use-zipkin"))]
    {
        use std::time::Duration;

        use tracing::{debug, debug_span, Instrument};

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

    info!(message);
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

/// Export any spans not exported yet.
pub fn shutdown() {
    info!("Shutting down");
    #[cfg(any(feature = "use-jaeger", feature = "use-zipkin"))]
    opentelemetry::global::shutdown_tracer_provider();
}
