mod common;

use color_eyre::Result;
use common::*;
use serial_keel::{client::ClientHandle, events::user, events::Event, events::TimestampedEvent};
use tracing::{debug, info};

macro_rules! assert_next_user_event {
    ($reader:ident, $event:pat) => {
        debug!("Sitting here waiting");
        let user_event = $reader.next_event().await;
        debug!(?user_event, "Event gotten");

        assert!(matches!(
            user_event,
            TimestampedEvent {
                inner: Event::User(user::UserEvent {
                    user: _,
                    event: $event,
                }),
                timestamp: _,
            }
        ));
    };
}

#[tokio::test]
async fn events() -> Result<()> {
    // serial_keel::logging::init().await;

    let port = start_server().await;

    // Event consumer
    let mut client_1 = ClientHandle::new("localhost", port).await?;
    let mut reader = client_1.observe_events().await?;

    info!("User event consumer started");

    info!("Client connecting");
    let mut some_client = ClientHandle::new("localhost", port).await?;
    info!("OK, checking event");
    assert_next_user_event!(reader, user::Event::Connected);

    info!("Client will ask to observe mock");
    some_client.observe_mock("john").await?;
    info!("OK, checking event..");
    assert_next_user_event!(reader, user::Event::Observing(_));

    info!("Client will ask to control mock");
    some_client.control_mock("john2").await?;
    info!("OK, checking event..");
    assert_next_user_event!(reader, user::Event::InControlOf(_));

    drop(some_client);
    info!("Client dropped");

    // TODO: It's implementation defined that this is the order of events,
    // but it's the order that the current implementation uses.
    // Consider making this order explicit.
    info!("Checking event: No longer observing");
    assert_next_user_event!(reader, user::Event::NoLongerObserving(_));

    info!("Checking event: No longer in control");
    assert_next_user_event!(reader, user::Event::NoLongerInControlOf(_));

    info!("Checking event: Disconnected");
    assert_next_user_event!(reader, user::Event::Disconnected);

    Ok(())
}

#[cfg(feature = "mocks-share-endpoints")]
#[tokio::test]
async fn user_gets_control_means_no_longer_in_queue_event() -> Result<()> {
    // serial_keel::logging::init().await;

    let port = start_server().await;

    // User event consumer
    let mut event_observer = ClientHandle::new("localhost", port).await?;
    let mut reader = event_observer.observe_events().await?;

    let mut cli_1 = ClientHandle::new("localhost", port).await?;
    assert_next_user_event!(reader, user::Event::Connected);
    info!("Cli1 conn");

    cli_1.control_mock("shared-foo").await?;
    assert_next_user_event!(reader, user::Event::InControlOf(_));
    info!("Cli1 control");

    let mut cli_2 = ClientHandle::new("localhost", port).await?;
    assert_next_user_event!(reader, user::Event::Connected);
    info!("Cli2 conn");

    let tx = cli_2.tx_mut();
    tx.control_mock("shared-foo").await?;

    // cli_2.control_mock("shared-foo").await?;
    assert_next_user_event!(reader, user::Event::InQueueFor(_));
    info!("Cli2 queue");

    drop(cli_1);

    // Expecting events:
    //  * cli_1 no longer in control of
    //  * cli_1 disconnected
    //  * cli_2 no longer in queue of
    //  * cli_2 controlling

    // cli 1
    assert_next_user_event!(reader, user::Event::NoLongerInControlOf(_));
    assert_next_user_event!(reader, user::Event::Disconnected);

    // cli 2
    assert_next_user_event!(reader, user::Event::NoLongerInQueueOf(_));
    assert_next_user_event!(reader, user::Event::InControlOf(_));

    Ok(())
}
