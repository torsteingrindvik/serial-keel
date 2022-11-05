use serial_keel::{logging, server};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    logging::init().await;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C, quitting")
        }
        _ = server::run_on_port(3123) => {
            error!("Server returned")
        }
    }

    logging::shutdown();
}
