use std::io::BufRead;
use std::io::BufReader;
use std::time::Duration;

use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::actions::Response;
use serial_keel::{
    actions::{self, Action},
    endpoint::EndpointLabel,
    error::Error,
};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;
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

async fn receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<actions::ResponseResult> {
    let response = timeout(Duration::from_secs(1), client.next())
        .await?
        .ok_or_else(|| color_eyre::eyre::eyre!("Stream closed"))??;

    let response = response.to_text()?;
    let response = serde_json::from_str(response)?;

    Ok(response)
}

async fn send_receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    to_send: String,
) -> Result<actions::ResponseResult> {
    client.send(tungstenite::Message::Text(to_send)).await?;
    receive(client).await
}

#[tokio::test]
async fn can_mock_lorem_ipsum_word_at_a_time() -> Result<()> {
    let mut client = connect().await?;

    let label = EndpointLabel::Mock("lorem_one_word".into());
    let request = Action::observe(&label).serialize();

    send_receive(&mut client, request).await??;

    for word in lipsum::lipsum_from_seed(1000, 123).split_ascii_whitespace() {
        let request = Action::write(&label, word.into()).serialize();
        send_receive(&mut client, request).await??;

        let response = receive(&mut client).await??;

        let expected_response = Response::Message {
            endpoint: label.clone(),
            message: word.into(),
        };
        assert_eq!(response, expected_response);
        // dbg!(&response);
    }

    Ok(())
}

// #[tokio::test]
// async fn can_mock_lorem_ipsum_inject_1000_words() -> Result<()> {
//     let mut client = connect().await?;

//     let _response = send_receive(&mut client, "hi".into()).await?;

//     let words = lipsum::lipsum_from_seed(1000, 123);

//     Ok(())
// }
