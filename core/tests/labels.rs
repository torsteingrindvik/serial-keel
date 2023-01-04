mod common;

use color_eyre::Result;
use serial_keel::{
    actions::{self, Action, Response},
    config::{Config, ConfigEndpoint, Group},
    endpoint::{EndpointId, Label, LabelledEndpointId, Labels},
    error::Error,
};

use common::*;

#[tokio::test]
async fn can_ask_for_non_existent() -> Result<()> {
    let mut client = start_server_and_connect().await?;

    let response = send_receive(&mut client, Action::control_any(&["foo"]).serialize()).await?;
    assert!(matches!(response, Err(Error::NoMatchingEndpoints(_))));

    Ok(())
}

#[tokio::test]
async fn can_control_label() -> Result<()> {
    let mut config = Config::default();
    let label = "bar";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("Mock1".into()),
        labels: label.into(),
    });

    let mut client = connect(start_server_with_config(config).await).await?;

    let response = send_receive(&mut client, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn two_labelled_endpoints_and_two_users_means_no_queue() -> Result<()> {
    let mut config = Config::default();
    let label = "baz";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("Mock1".into()),
        labels: label.into(),
    });
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("Mock2".into()),
        labels: label.into(),
    });

    let port = start_server_with_config(config).await;
    let mut client_1 = connect(port).await?;
    let mut client_2 = connect(port).await?;

    let response = send_receive(&mut client_1, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    let response = send_receive(&mut client_2, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn two_labelled_endpoints_and_one_user() -> Result<()> {
    let mut config = Config::default();
    let label = "qux";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("Mock1".into()),
        labels: label.into(),
    });
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("Mock2".into()),
        labels: label.into(),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    let response = send_receive(&mut client, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    let response = send_receive(&mut client, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn two_labelled_endpoints_can_still_use_specific_names() -> Result<()> {
    let mut config = Config::default();
    let label_str = "abc-1";
    let label = Label::new(label_str);

    config.auto_open_serial_ports = false;

    let mock1 = EndpointId::Mock("Mock1".into());
    config.endpoints.push(ConfigEndpoint {
        id: mock1.clone(),
        labels: Labels::from_iter([&label]),
    });
    let lmock1 = LabelledEndpointId {
        id: mock1.clone(),
        labels: Some(Labels::from_iter([&label])),
    };

    let mock2 = EndpointId::Mock("Mock2".into());
    config.endpoints.push(ConfigEndpoint {
        id: mock2.clone(),
        labels: label.into(),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    match send_receive(&mut client, Action::control_any(&[label_str]).serialize()).await?? {
        Response::Sync(actions::Sync::ControlGranted(control)) => {
            // Since we want a specific endpoint after a label we need to ask for the
            // available one.
            let next = if control[0] == lmock1 { mock2 } else { mock1 };

            let response = send_receive(&mut client, Action::control(&next).serialize()).await??;
            assert_granted!(response);
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
        id: EndpointId::Mock("ccdl-Mock1".into()),
        labels: label_1.into(),
    });
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("ccdl-Mock2".into()),
        labels: label_2.into(),
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    let response = send_receive(&mut client, Action::control_any(&[label_1]).serialize()).await??;
    assert_granted!(response);

    let response = send_receive(&mut client, Action::control_any(&[label_2]).serialize()).await??;
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn granted_labelled_endpoint_is_freed_when_user_drops() -> Result<()> {
    let mut config = Config::default();
    let label = "should_drop";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("sd".into()),
        labels: Labels::from_iter([label]),
    });
    let port = start_server_with_config(config).await;
    let mut client_1 = connect(port).await?;
    let mut client_2 = connect(port).await?;

    let response = send_receive(&mut client_1, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);
    drop(client_1);

    let response = send_receive(&mut client_2, Action::control_any(&[label]).serialize()).await??;
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn user_is_informed_of_endpoint_labels() -> Result<()> {
    // An endpoint may have a group label AND individual labels for the endpoints.
    // If the user asks for access to a group, they should get to see the specific labels too.

    let mut config = Config::default();
    let group_label = Label::new("group-label");
    let endpoint_label = Label::new("endpoint-label");

    config.auto_open_serial_ports = false;
    config.groups.push(Group {
        labels: group_label.clone().into(),
        endpoints: vec![ConfigEndpoint {
            id: EndpointId::mock("glmock"),
            labels: Labels::from_iter([endpoint_label.clone()]),
        }],
    });

    let port = start_server_with_config(config).await;
    let mut client = connect(port).await?;

    let response = send_receive(
        &mut client,
        Action::control_any(&[&group_label]).serialize(),
    )
    .await??;
    dbg!(&response);

    match response {
        Response::Sync(actions::Sync::ControlGranted(granted)) => {
            let grant = &granted[0];

            assert!(grant
                .labels
                .as_ref()
                .unwrap()
                .iter()
                .any(|l| l == &group_label));

            assert!(grant
                .labels
                .as_ref()
                .unwrap()
                .iter()
                .any(|l| l == &endpoint_label));
        }
        _ => unreachable!(),
    };

    Ok(())
}

#[tokio::test]
async fn multiple_label_endpoint_is_found_via_subset() -> Result<()> {
    let mut config = Config::default();
    let label_1 = "john";
    let label_2 = "mary";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("MockManyLabels".into()),
        labels: Labels::from_iter([label_1, label_2]),
    });

    let mut client = connect(start_server_with_config(config).await).await?;

    let response = send_receive(&mut client, Action::control_any(&[label_1]).serialize()).await??;
    dbg!(&response);
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn multiple_label_endpoint_is_found_via_equal_set() -> Result<()> {
    let mut config = Config::default();
    let label_1 = "john";
    let label_2 = "mary";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("MockManyLabels2".into()),
        labels: Labels::from_iter([label_1, label_2]),
    });

    let mut client = connect(start_server_with_config(config).await).await?;

    let response = send_receive(
        &mut client,
        Action::control_any(&[label_2, label_1]).serialize(),
    )
    .await??;
    dbg!(&response);
    assert_granted!(response);

    Ok(())
}

#[tokio::test]
async fn single_label_endpoint_is_not_matched_via_superset() -> Result<()> {
    let mut config = Config::default();
    let label_1 = "john";
    let label_2 = "mary";

    config.auto_open_serial_ports = false;
    config.endpoints.push(ConfigEndpoint {
        id: EndpointId::Mock("MockManyLabels3".into()),
        labels: label_2.into(),
    });

    let mut client = connect(start_server_with_config(config).await).await?;

    let response = send_receive(
        &mut client,
        Action::control_any(&[label_2, label_1]).serialize(),
    )
    .await?;
    assert_result_error!(response, Error::NoMatchingEndpoints(_));

    Ok(())
}
