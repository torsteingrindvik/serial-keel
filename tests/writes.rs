use color_eyre::Result;
use serial_keel::{
    actions::{Action, Response},
    endpoint::EndpointLabel,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, debug_span, info, Instrument};

use common::{receive, send_receive, start_server_and_connect};

mod common;

#[tokio::test]
async fn can_mock_lorem_ipsum_word_at_a_time() -> Result<()> {
    serial_keel::logging::init().await;

    info!("Connecting");
    let mut client = start_server_and_connect().await?;
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
    let mut client = start_server_and_connect().await?;
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
