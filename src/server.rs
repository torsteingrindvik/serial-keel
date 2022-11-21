use std::net::SocketAddr;

use axum::{routing::get, Extension, Router};
use tokio::sync::oneshot;
use tracing::info;

use crate::{config::Config, control_center::ControlCenterHandle, websocket};

async fn run(config: Config, port: Option<u16>, allocated_port: Option<oneshot::Sender<u16>>) {
    config.validate().expect("Configuration must be valid");

    let cc_handle = ControlCenterHandle::new(&config);

    let app = Router::new()
        .route("/ws", get(websocket::ws_handler))
        // Each websocket needs to be able to reach the control center
        .layer(Extension(cc_handle));

    let addr = SocketAddr::from(([0, 0, 0, 0], port.unwrap_or(0)));
    let server =
        axum::Server::bind(&addr).serve(app.into_make_service_with_connect_info::<SocketAddr>());
    let addr = server.local_addr();

    if let Some(port_reply) = allocated_port {
        port_reply
            .send(addr.port())
            .expect("The receiver of which port was allocated should not be dropped");
    }

    info!("listening on {}", addr);

    server.await.unwrap();
}

/// Start the server on an arbitrary available port.
/// The port allocated will be sent on the provided channel.
pub async fn run_any_port(config: Config, allocated_port: oneshot::Sender<u16>) {
    run(config, None, Some(allocated_port)).await
}

/// Start the server on the given port.
pub async fn run_on_port(config: Config, port: u16) {
    run(config, Some(port), None).await
}
