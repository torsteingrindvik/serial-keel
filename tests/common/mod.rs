#![allow(dead_code)]

use std::time::Duration;

use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::actions;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::info;

pub async fn connect() -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let (port_tx, port_rx) = oneshot::channel();

    tokio::spawn(async move { serial_keel::server::run_any_port(port_tx).await });
    let port = port_rx
        .await
        .expect("Server should reply with allocated port");

    info!("Connecting to server on port {port}");
    let (stream, http_response) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws")).await?;

    assert_eq!(http_response.status(), StatusCode::SWITCHING_PROTOCOLS);

    Ok(stream)
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
