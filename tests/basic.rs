use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::actions;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::debug;

async fn connect() -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let (port_tx, port_rx) = oneshot::channel();

    tokio::spawn(async move { serial_keel::server::run_any_port(port_tx).await });
    let port = port_rx
        .await
        .expect("Server should reply with allocated port");

    let (stream, http_response) =
        tokio_tungstenite::connect_async(format!("ws://localhost:{port}/ws")).await?;

    assert_eq!(http_response.status(), StatusCode::SWITCHING_PROTOCOLS);

    Ok(stream)
}

#[tokio::test]
async fn can_connect() -> Result<()> {
    connect().await?;

    Ok(())
}

#[tokio::test]
async fn can_send_and_receive() -> Result<()> {
    let mut client = connect().await?;

    client.send(tungstenite::Message::Text("hi".into())).await?;

    let response = client
        .next()
        .await
        .ok_or_else(|| color_eyre::eyre::eyre!("Stream closed"))??;

    // Generally, responses should be text
    assert!(matches!(response, tungstenite::Message::Text(_)));

    debug!("Got response {response:?}");

    Ok(())
}

#[tokio::test]
async fn non_json_request_is_bad() -> Result<()> {
    let mut client = connect().await?;

    client.send(tungstenite::Message::Text("hi".into())).await?;

    let response = client
        .next()
        .await
        .ok_or_else(|| color_eyre::eyre::eyre!("Stream closed"))??;

    let response = response.to_text()?;

    let response: actions::Response = serde_json::from_str(response)?;

    assert!(matches!(
        response,
        actions::Response::CouldNotDeserializeJsonToAction
    ));

    Ok(())
}
