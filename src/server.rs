use axum::routing::get;
use axum::{Extension, Router};
use std::net::SocketAddr;
use tokio::sync::oneshot;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::debug;

use crate::{control_center::ControlCenter, websocket};

async fn run(port: Option<u16>, allocated_port: Option<oneshot::Sender<u16>>) {
    let cc_handle = ControlCenter::run();

    let app = Router::new()
        .route("/ws", get(websocket::ws_handler))
        // Each websocket needs to be able to reach the control center
        .layer(Extension(cc_handle))
        // Logging so we can see whats going on
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(0)));
    let server = axum::Server::bind(&addr).serve(app.into_make_service());
    let addr = server.local_addr();

    if let Some(port_reply) = allocated_port {
        port_reply
            .send(addr.port())
            .expect("The receiver of which port was allocated should not be dropped");
    }

    debug!("listening on {}", addr);

    server.await.unwrap();
}

/// Start the server on an arbitrary available port.
/// The port allocated will be sent on the provided channel.
pub async fn run_any_port(allocated_port: oneshot::Sender<u16>) {
    run(None, Some(allocated_port)).await
}

/// Start the server on the given port.
pub async fn run_on_port(port: u16) {
    run(Some(port), None).await
}
