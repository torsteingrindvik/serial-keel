use color_eyre::Result;
use common::*;
use serial_keel::client::ClientHandle;
use serial_keel::endpoint::{EndpointId, LabelledEndpointId};
use serial_keel::error::Error;

mod common;

#[tokio::test]
async fn can_use_mock() -> Result<()> {
    serial_keel::logging::init().await;

    let port = start_server().await;

    let mut client = ClientHandle::new("localhost", port).await?;

    let mock_1 = "Hi there";
    let observing = client.observe_mock(mock_1).await?;
    let e = &observing[0];

    let in_control_of = client.control_mock(mock_1).await?;
    assert_eq!(
        in_control_of,
        vec![LabelledEndpointId::new(&EndpointId::mock(mock_1))]
    );

    let mock_2 = "Hi foo";
    let in_control_of = client.control_mock(mock_2).await?;
    assert_eq!(
        in_control_of,
        vec![LabelledEndpointId::new(&EndpointId::mock(mock_2))]
    );

    let not_an_endpoint = LabelledEndpointId::new(&EndpointId::mock("Nope"));
    let err = client.next_message(&not_an_endpoint).await;
    assert!(matches!(err, Err(Error::NoSuchEndpoint(_))));

    let message = "This is a message\nAnd so on";
    client.write(e, message).await?;

    let (m1, m2) = message.split_once('\n').unwrap();

    let received = client.next_message(e).await?;
    assert_eq!(m1, received.as_str());

    let received = client.next_message(e).await?;
    assert_eq!(m2, received.as_str());

    Ok(())
}
