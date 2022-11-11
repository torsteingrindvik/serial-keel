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

#[tokio::test]
async fn two_labelled_endpoints_and_two_users_means_no_queue() -> Result<()> {
    let mut config = Config::default();
    let label = "baz";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("Mock1".into()),
        label: Some(Label::new(label)),
    });
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("Mock2".into()),
        label: Some(Label::new(label)),
    });

    let port = start_server_with_config(config).await;
    let mut client_1 = connect(port).await?;
    let mut client_2 = connect(port).await?;

    let response = send_receive(&mut client_1, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    let response = send_receive(&mut client_2, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    Ok(())
}

#[tokio::test]
async fn two_labelled_endpoints_and_one_user() -> Result<()> {
    let mut config = Config::default();
    let label = "qux";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("Mock1".into()),
        label: Some(Label::new(label)),
    });
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("Mock2".into()),
        label: Some(Label::new(label)),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    let response = send_receive(&mut client, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    let response = send_receive(&mut client, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    Ok(())
}
