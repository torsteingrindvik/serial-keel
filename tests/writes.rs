use color_eyre::Result;
use common::{receive, send_receive, start_server_and_connect};
use serial_keel::{
    actions::{Action, Response},
    endpoint::EndpointId,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, debug_span, info, Instrument};

mod common;

#[tokio::test]
async fn can_mock_lorem_ipsum_word_at_a_time() -> Result<()> {
    info!("Connecting");
    let mut client = start_server_and_connect().await?;
    info!("Connected");

    let mock_name = "lorem_one_word";
    let id = EndpointId::Mock(mock_name.into());
    let request = Action::control(&id).serialize();

    info!("Requesting control");
    send_receive(&mut client, request).await??;

    info!("Requesting observe");
    let request = Action::observe_mock(mock_name).serialize();
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
        let request = Action::write(&id, word.into()).serialize();

        let response = one_msg(&mut client, request)
            .instrument(debug_span!("one-msg"))
            .await?;

        let expected_response = Response::Message {
            endpoint: id.clone(),
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

    let id = EndpointId::Mock("lorem_many_words".into());
    let request = Action::control(&id).serialize();
    send_receive(&mut client, request).await??;

    let request = Action::observe_mock("lorem_many_words").serialize();
    send_receive(&mut client, request).await??;

    let words = lipsum::lipsum_from_seed(1000, 123);
    let request = Action::write(&id, words.clone()).serialize();
    send_receive(&mut client, request).await??;

    let response = receive(&mut client).await??;

    // Just to see that the mock endpoint is cleaned up
    drop(client);

    let expected_response = Response::Message {
        endpoint: id.clone(),
        message: words,
    };
    assert_eq!(response, expected_response);

    Ok(())
}
