/// There was an issue in some async Python client mocks.
/// See mock.ron for the server setup.
///
/// Here we try to recreate this use case:
///     1. A "control any" command is sent. Two matching labels are needed: "5340", and "mock".
///     2. The "non-secure" endpoint should be observed.
///     3. Then an entire file is dumped into the endpoint
///     4. The contents should then be received line by line.
///
/// Several clients should try to do this at the same time.
use color_eyre::Result;
use serial_keel::client::ClientHandle;

use crate::common;
use serial_keel::config::Config;

async fn run_a_client(port: u16) -> Result<()> {
    let mut client = ClientHandle::new("localhost", port).await?;

    // 1.
    let mut endpoints = client.control_any(&["5340", "mock"]).await?;
    let mut non_secure_endpoint = endpoints
        .remove_writer_with_labels(&("non-secure".into()))
        .unwrap();

    // 2.
    let mock_id = non_secure_endpoint.endpoint_id().id.as_mock().unwrap();
    let mut observer = client.observe_mock(mock_id).await?;

    // 3.
    let expected = include_str!("CRYPTO.log");
    non_secure_endpoint.write(expected).await?;

    // 4.
    loop {
        let msg = observer.next_message().await?;
        if msg.as_str().contains("Entering standby") {
            break;
        }
    }

    Ok(())
}

#[tokio::test]
async fn test() -> Result<()> {
    let config = Config::deserialize(include_str!("mock.ron"));
    let port = common::start_server_with_config(config).await;

    let (r1, r2, r3, r4, r5) = tokio::join!(
        run_a_client(port),
        run_a_client(port),
        run_a_client(port),
        run_a_client(port),
        run_a_client(port)
    );
    r1?;
    r2?;
    r3?;
    r4?;
    r5?;

    Ok(())
}
