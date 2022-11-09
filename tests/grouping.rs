mod common;

// Feature: Can't test grouping if endpoints are not shared
#[cfg(feature = "mocks-share-endpoints")]
mod grouping {
    use color_eyre::Result;
    use pretty_assertions::assert_eq;
    use serial_keel::{
        actions::{Action, Response},
        endpoint::EndpointLabel,
    };

    use super::common::*;

    #[tokio::test]
    async fn control_one_means_control_all() -> Result<()> {
        let m1 = EndpointLabel::mock("Mock1");
        let m2 = EndpointLabel::mock("Mock2");

        let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;
        let mut client = connect(port).await?;

        let response = send_receive(&mut client, Action::control(&m1).serialize()).await??;
        assert_eq!(response, Response::ControlGranted(vec![m1]));

        let response = send_receive(&mut client, Action::control(&m2).serialize()).await?;
        dbg!(&response);

        assert!(matches!(
            response,
            Err(serial_keel::error::Error::SuperfluousRequest(_))
        ));

        Ok(())
    }
}
