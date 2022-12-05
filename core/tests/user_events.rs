mod common;

use color_eyre::Result;
use common::*;
use serial_keel::client::{ClientHandle, Event, UserEvent};
use tracing::info;

macro_rules! assert_next_event {
    ($reader:ident, $event:pat) => {
        let user_event = $reader.next_user_event().await;

        assert!(matches!(
            user_event,
            UserEvent {
                user: _,
                event: $event
            }
        ));
    };
}

#[tokio::test]
async fn user_events() -> Result<()> {
    serial_keel::logging::init().await;

    let port = start_server().await;

    // User event consumer
    let mut client_1 = ClientHandle::new("localhost", port).await?;
    let mut reader = client_1.observe_user_events().await?;

    info!("User event consumer started");

    info!("Client connecting");
    let mut some_client = ClientHandle::new("localhost", port).await?;
    info!("OK, checking event");
    assert_next_event!(reader, Event::Connected);

    info!("Client will ask to observe mock");
    some_client.observe_mock("john").await?;
    info!("OK, checking event..");
    assert_next_event!(reader, Event::Observing(_));

    info!("Client will ask to control mock");
    some_client.control_mock("john2").await?;
    info!("OK, checking event..");
    assert_next_event!(reader, Event::InControlOf(_));

    drop(some_client);
    info!("Client dropped");

    // TODO: It's implementation defined that this is the order of events,
    // but it's the order that the current implementation uses.
    // Consider making this order explicit.
    info!("Checking event: No longer observing");
    assert_next_event!(reader, Event::NoLongerObserving(_));

    info!("Checking event: No longer in control");
    assert_next_event!(reader, Event::NoLongerInControlOf(_));

    info!("Checking event: Disconnected");
    assert_next_event!(reader, Event::Disconnected);

    Ok(())
}
