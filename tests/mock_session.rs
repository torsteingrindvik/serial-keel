use std::time::Duration;

use axum::http::StatusCode;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use serial_keel::actions::Response;
use serial_keel::{
    actions::{self, Action},
    endpoint::EndpointLabel,
};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::debug_span;
use tracing::Instrument;
use tracing::{debug, info};

async fn connect() -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
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

async fn receive(
    client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<actions::ResponseResult> {
    let response = timeout(Duration::from_secs(5), client.next())
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
    serial_keel::logging::init().await;

    info!("Connecting");
    let mut client = connect().await?;
    info!("Connected");

    let label = EndpointLabel::Mock("lorem_one_word".into());
    let request = Action::control(&label).serialize();

    info!("Requesting observe");
    send_receive(&mut client, request).await??;

    info!("Observing; starting lipsum words");

    async fn one_msg(
        client: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        request: String,
    ) -> Result<Response> {
        debug!(%request, "Mocking");
        send_receive(client, request).await??;

        debug!("Now awaiting that back");
        // For some reason this always takes ~40 ms?
        let response = receive(client).await??;
        debug!("Done");

        Ok(response)
    }

    for word in lipsum::lipsum_from_seed(100, 123).split_ascii_whitespace() {
        debug!(?word, "word");
        let request = Action::write(&label, word.into()).serialize();

        let response = one_msg(&mut client, request)
            .instrument(debug_span!("one-msg"))
            .await?;

        let expected_response = Response::Message {
            endpoint: label.clone(),
            message: word.into(),
        };
        assert_eq!(response, expected_response);
    }

    Ok(())
}

#[tokio::test]
async fn can_mock_lorem_ipsum_inject_1000_words() -> Result<()> {
    info!("Connecting");
    let mut client = connect().await?;
    info!("Connected");

    let label = EndpointLabel::Mock("lorem_many_words".into());
    let request = Action::control(&label).serialize();

    send_receive(&mut client, request).await??;

    let words = lipsum::lipsum_from_seed(1000, 123);
    let request = Action::write(&label, words.clone()).serialize();
    send_receive(&mut client, request).await??;

    let response = receive(&mut client).await??;

    // Just to see that the mock endpoint is cleaned up
    drop(client);

    let expected_response = Response::Message {
        endpoint: label.clone(),
        message: words,
    };
    assert_eq!(response, expected_response);

    Ok(())
}
