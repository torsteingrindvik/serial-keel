use serial_keel::{config::Config, logging, server};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    logging::init().await;

    // TODO: From CLI
    let config = Config::default();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C, quitting")
        }
        _ = server::run_on_port(config, 3123) => {
            error!("Server returned")
        }
    }

    logging::shutdown();
}
