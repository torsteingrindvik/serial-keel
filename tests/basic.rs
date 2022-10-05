use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::{
    actions::{self, Action},
    endpoint::EndpointLabel,
    error::Error,
};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

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

async fn send_receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    to_send: String,
) -> Result<actions::ResponseResult> {
    client.send(tungstenite::Message::Text(to_send)).await?;

    let response = client
        .next()
        .await
        .ok_or_else(|| color_eyre::eyre::eyre!("Stream closed"))??;

    let response = response.to_text()?;
    let response = serde_json::from_str(response)?;

    Ok(response)
}

#[tokio::test]
async fn can_send_and_receive() -> Result<()> {
    let mut client = connect().await?;
    let _response = send_receive(&mut client, "hi".into()).await?;

    Ok(())
}

#[tokio::test]
async fn non_json_request_is_bad() -> Result<()> {
    let mut client = connect().await?;
    let response = send_receive(&mut client, "hi".into()).await?;

    assert!(matches!(response, Result::Err(Error::BadRequest(_))));

    Ok(())
}

#[tokio::test]
async fn non_existing_endpoint_observe_is_bad() -> Result<()> {
    let mut client = connect().await?;

    let request = Action::Observe(EndpointLabel::Mock("mock".into())).serialize();
    let response = send_receive(&mut client, request).await?;

    assert!(matches!(response, Result::Err(Error::BadRequest(_))));

    Ok(())
}
