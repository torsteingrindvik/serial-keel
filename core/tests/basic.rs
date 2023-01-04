use color_eyre::Result;
use common::{send_receive, start_server_and_connect};
use serial_keel::{
    actions::{Action, Response},
    endpoint::EndpointId,
    error::Error,
};

mod common;

#[tokio::test]
async fn can_connect() -> Result<()> {
    start_server_and_connect().await?;

    Ok(())
}

#[tokio::test]
async fn can_send_and_receive() -> Result<()> {
    let mut client = start_server_and_connect().await?;
    let _response = send_receive(&mut client, "hi".into()).await?;

    Ok(())
}

#[tokio::test]
async fn non_json_request_is_bad() -> Result<()> {
    let mut client = start_server_and_connect().await?;
    let response = send_receive(&mut client, "hi".into()).await?;

    assert!(matches!(response, Result::Err(Error::BadJson { .. })));

    Ok(())
}

#[tokio::test]
async fn non_existing_mock_endpoint_observe_is_ok() -> Result<()> {
    let mut client = start_server_and_connect().await?;

    let request = Action::observe_mock("non_existing_mock_endpoint_observe_is_ok").serialize();
    let response = send_receive(&mut client, request).await??;

    assert_observing!(response);

    Ok(())
}

#[tokio::test]
async fn observe_same_twice_is_bad() -> Result<()> {
    let mut client = start_server_and_connect().await?;

    let request = Action::observe_mock("twice").serialize();
    let response = send_receive(&mut client, request).await??;
    assert_observing!(response);

    let request = Action::observe_mock("twice").serialize();
    let response = send_receive(&mut client, request).await?;

    assert_result_error!(response, Error::SuperfluousRequest(_));

    Ok(())
}

#[tokio::test]
async fn observe_mock_and_write_is_bad_no_control() -> Result<()> {
    let mut client = start_server_and_connect().await?;

    let id = EndpointId::mock("some-mock");

    let request = Action::Observe(id.clone()).serialize();
    let response = send_receive(&mut client, request).await??;
    assert_observing!(response);

    let request = Action::write(&id, "Hi there".into()).serialize();
    let response = send_receive(&mut client, request).await?;

    assert_result_error!(response, Error::NoPermit(_));

    Ok(())
}
