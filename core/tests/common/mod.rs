#![allow(dead_code)]

use std::time::Duration;

use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::{
    actions,
    config::{Config, Group},
};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::info;

#[macro_export]
macro_rules! assert_granted {
    ($response:ident) => {
        assert!(matches!(
            $response,
            Response::Sync(serial_keel::actions::Sync::ControlGranted(_))
        ));
    };

    ($response:ident, $lid:ident) => {
        assert_eq!(
            $response,
            Response::Sync(serial_keel::actions::Sync::ControlGranted(vec![
                $lid.clone()
            ]))
        );
    };
}

#[macro_export]
macro_rules! assert_queued {
    ($response:ident) => {
        assert!(matches!(
            $response,
            Response::Sync(serial_keel::actions::Sync::ControlQueue(_))
        ));
    };

    ($response:ident, $lid:ident) => {
        assert_eq!(
            $response,
            Response::Sync(serial_keel::actions::Sync::ControlQueue(vec![$lid.clone()]))
        );
    };
}

#[macro_export]
macro_rules! assert_observing {
    ($response:ident) => {
        assert!(matches!(
            $response,
            Response::Sync(serial_keel::actions::Sync::Observing(_))
        ));
    };
}

#[macro_export]
macro_rules! assert_result_error {
    ($response:ident, $e:pat) => {
        assert!(matches!($response, Result::Err($e)));
    };
}

pub async fn start_server() -> u16 {
    start_server_with_config(Config::default()).await
}

pub async fn start_server_with_config(config: Config) -> u16 {
    let (port_tx, port_rx) = oneshot::channel();

    tokio::spawn(async move { serial_keel::server::run_any_port(config, port_tx).await });
    port_rx
        .await
        .expect("Server should reply with allocated port")
}

pub async fn start_server_with_group(group: Group) -> u16 {
    start_server_with_config(Config {
        groups: vec![group],
        endpoints: vec![],
        ignore_unavailable_endpoints: false,
    })
    .await
}

pub async fn connect(port: u16) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    info!("Connecting to server on port {port}");
    let (stream, http_response) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/client")).await?;

    assert_eq!(http_response.status(), StatusCode::SWITCHING_PROTOCOLS);

    Ok(stream)
}

pub async fn start_server_and_connect() -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let port = start_server().await;
    connect(port).await
}

pub async fn receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<actions::ResponseResult> {
    let response = timeout(Duration::from_secs(5), client.next())
        .await?
        .ok_or_else(|| color_eyre::eyre::eyre!("Stream closed"))??;

    let response = response.to_text()?;
    let response = serde_json::from_str(response)?;

    Ok(response)
}

pub async fn send_receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    to_send: String,
) -> Result<actions::ResponseResult> {
    client.send(tungstenite::Message::Text(to_send)).await?;
    receive(client).await
}
