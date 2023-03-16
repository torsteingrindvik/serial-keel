use color_eyre::Result;
use common::*;
use serial_keel::client::ClientHandle;
use serial_keel::endpoint::{EndpointId, LabelledEndpointId};
use tracing::debug;

mod common;

#[tokio::test]
async fn can_use_mock() -> Result<()> {

    let port = start_server().await;

    let mut client = ClientHandle::new("localhost", port).await?;

    let mock_1 = "Hi there";
    debug!(%mock_1,  "Observe");
    let mut observing = client.observe_mock(mock_1).await?;
    let id = observing.endpoint_id();
    assert_eq!(id, &LabelledEndpointId::new(&EndpointId::mock(mock_1)));

    debug!(%mock_1,  "Control");
    let mut in_control_of = client.control_mock(mock_1).await?;
    let id = in_control_of.endpoint_id();
    assert_eq!(id, &LabelledEndpointId::new(&EndpointId::mock(mock_1)));

    let mock_1_writer = &mut in_control_of;

    let mock_2 = "Hi foo";
    debug!(%mock_2,  "Control");
    let in_control_of = client.control_mock(mock_2).await?;
    let id = in_control_of.endpoint_id();
    assert_eq!(id, &LabelledEndpointId::new(&EndpointId::mock(mock_2)));

    let message = "This is a message\nAnd so on";
    debug!(%mock_1,  "Write");
    mock_1_writer.write(message).await?;

    let (m1, m2) = message.split_once('\n').unwrap();

    debug!(%mock_1,  "Next message");
    let received = observing.next_message().await;
    assert_eq!(m1, received.as_str());

    debug!(%mock_1,  "Next message");
    let received = observing.next_message().await;
    assert_eq!(m2, received.as_str());

    drop(client);

    Ok(())
}
