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

#[tokio::test]
async fn two_labelled_endpoints_can_still_use_specific_names() -> Result<()> {
    let mut config = Config::default();
    let label = "abc-1";

    config.auto_open_serial_ports = false;

    let mock1 = EndpointId::Mock("Mock1".into());
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: mock1.clone(),
        label: Some(Label::new(label)),
    });

    let mock2 = EndpointId::Mock("Mock2".into());
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: mock2.clone(),
        label: Some(Label::new(label)),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    match send_receive(&mut client, Action::control_any(label).serialize()).await?? {
        Response::ControlGranted(control) => {
            // Since we want a specific endpoint after a label we need to ask for the
            // available one.
            let next = if control[0] == mock1 { mock2 } else { mock1 };

            let response = send_receive(&mut client, Action::control(&next).serialize()).await??;
            assert!(matches!(response, Response::ControlGranted(_)));
        }
        _ => unreachable!(),
    };

    Ok(())
}

#[tokio::test]
async fn can_control_different_labels() -> Result<()> {
    let mut config = Config::default();
    let label_1 = "ccdl-1";
    let label_2 = "ccdl-2";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("ccdl-Mock1".into()),
        label: Some(Label::new(label_1)),
    });
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("ccdl-Mock2".into()),
        label: Some(Label::new(label_2)),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    let response = send_receive(&mut client, Action::control_any(label_1).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    let response = send_receive(&mut client, Action::control_any(label_2).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    Ok(())
}

#[tokio::test]
async fn granted_labelled_endpoint_is_freed_when_user_drops() -> Result<()> {
    let mut config = Config::default();
    let label = "should_drop";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        endpoint_id: EndpointId::Mock("sd".into()),
        label: Some(Label::new(label)),
    });
    let port = start_server_with_config(config).await;
    let mut client_1 = connect(port).await?;
    let mut client_2 = connect(port).await?;

    let response = send_receive(&mut client_1, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));
    drop(client_1);

    let response = send_receive(&mut client_2, Action::control_any(label).serialize()).await??;
    assert!(matches!(response, Response::ControlGranted(_)));

    Ok(())
}
