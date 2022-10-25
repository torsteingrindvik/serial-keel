use serial_keel::{logging, server};

#[tokio::main]
async fn main() {
    logging::init().await;
    server::run_on_port(3000).await
}
