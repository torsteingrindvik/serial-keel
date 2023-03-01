use std::net::SocketAddr;

use axum::routing::post;
use axum::{response::IntoResponse, routing::get, Extension, Router};
use tokio::sync::oneshot;
use tracing::info;

use crate::{config::Config, control_center::ControlCenterHandle, websocket};
use semver::Version;
use tower::ServiceBuilder;

/// The default port to run the server on.
pub const DEFAULT_PORT: u16 = 3123;

async fn run(config: Config, port: Option<u16>, allocated_port: Option<oneshot::Sender<u16>>) {
    config.validate().expect("Configuration must be valid");

    let cc_handle = ControlCenterHandle::new(&config);

    let app = Router::new()
        .route("/client", get(websocket::ws_handler))
        .route("/config", get(show_config))
        .route("/version", get(show_version))
        .route("/exit", post(exit_server))
        .layer(
            ServiceBuilder::new()
                // Each websocket needs to be able to reach the control center
                .layer(Extension(cc_handle))
                // The serial-keel config should be known to the web server
                .layer(Extension(config.clone())),
        );

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

#[cfg(unix)]
async fn exit_server() {
    use nix::sys::signal;
    use nix::unistd::Pid;

    // The server has a graceful handler for SIGHUP.
    _ = nix::sys::signal::kill(Pid::this(), signal::Signal::SIGHUP);
}

#[cfg(not(unix))]
async fn exit_server() {
    // Not supported yet
}

async fn show_config(Extension(config): Extension<Config>) -> impl IntoResponse {
    config.serialize_pretty()
}

async fn show_version() -> impl IntoResponse {
    let version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("The server version should always be semver parsable!");
    serde_json::to_string_pretty(&version).unwrap()
}
