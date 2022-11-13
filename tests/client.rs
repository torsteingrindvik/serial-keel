use color_eyre::Result;
use common::*;
use serial_keel::{actions::Response, client::ClientHandle};

mod common;

#[tokio::test]
async fn can_use_mock() -> Result<()> {
    let port = start_server().await;

    let mut client = ClientHandle::new("localhost", port).await?;

    client.observe_mock("Hi there").await?;

    Ok(())
}
