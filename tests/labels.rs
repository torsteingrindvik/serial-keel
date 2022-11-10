// TODO: Have two matching labels, this would imply queueing for both.
// Need to check what happens when we are granted both, but one drops.

mod common;

use color_eyre::Result;
use serial_keel::{
    actions::{Action, Response},
    config::{Config, ConfigEndpoint},
    endpoint::{EndpointId, Label},
    error::Error,
};

use common::*;

#[tokio::test]
async fn can_ask_for_non_existent() -> Result<()> {
    let mut client = start_server_and_connect().await?;

    let response = send_receive(&mut client, Action::control_any("foo").serialize()).await?;
    assert!(matches!(response, Err(Error::NoMatchingEndpoints(_))));

    Ok(())
}

#[tokio::test]
async fn can_control_label() -> Result<()> {
    let mut config = Config::default();
    let label = "bar";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("Mock1".into()),
        label: Some(Label::new(label)),
    });

    let mut client = connect(start_server_with_config(config).await).await?;

    let response = send_receive(&mut client, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    Ok(())
}

// #[tokio::test]
// async fn can_ask_for_non_existant_label() -> Result<()> {
//     let m1 = EndpointId::mock("Mock31");
//     let m2 = EndpointId::mock("Mock32");

//     let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;

//     let mut client_1 = connect(port).await?;
//     let response = send_receive(&mut client_1, Action::control(&m1).serialize()).await??;
//     assert!(matches!(response, Response::ControlGranted(_)));

//     let mut client_2 = connect(port).await?;
//     let response = send_receive(&mut client_2, Action::observe(&m2).serialize()).await??;

//     assert!(matches!(response, Response::Ok));

//     Ok(())
// }
