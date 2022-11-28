mod common;

// Feature: Can't test grouping if endpoints are not shared
#[cfg(feature = "mocks-share-endpoints")]
mod grouping {
    use color_eyre::Result;
    use serial_keel::{
        actions::{self, Action, Response},
        endpoint::{EndpointId, LabelledEndpointId},
        error::Error,
    };

    use super::common::*;
    use crate::assert_granted;
    use crate::assert_observing;
    use crate::assert_result_error;

    #[tokio::test]
    async fn control_one_means_control_all() -> Result<()> {
        let m1 = EndpointId::mock("Mock11");
        let m2 = EndpointId::mock("Mock12");
        let lm1 = LabelledEndpointId::new(&m1);
        let lm2 = LabelledEndpointId::new(&m2);

        let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;
        let mut client = connect(port).await?;

        match send_receive(&mut client, Action::control(&m1).serialize()).await?? {
            Response::Sync(actions::Sync::ControlGranted(granted)) => {
                assert!(granted.contains(&lm1));
                assert!(granted.contains(&lm2));
            }
            _ => unreachable!(),
        };

        let response = send_receive(&mut client, Action::control(&m2).serialize()).await?;

        assert_result_error!(response, Error::SuperfluousRequest(_));

        Ok(())
    }

    #[tokio::test]
    async fn control_one_means_second_user_cannot_control_other_in_group() -> Result<()> {
        let m1 = EndpointId::mock("Mock21");
        let m2 = EndpointId::mock("Mock22");
        let lm1 = LabelledEndpointId::new(&m1);
        let lm2 = LabelledEndpointId::new(&m2);

        let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;
        let mut client_1 = connect(port).await?;

        match send_receive(&mut client_1, Action::control(&m1).serialize()).await?? {
            Response::Sync(actions::Sync::ControlGranted(granted)) => {
                assert!(granted.contains(&lm1));
                assert!(granted.contains(&lm2));
            }
            _ => unreachable!(),
        };

        let mut client_2 = connect(port).await?;

        match send_receive(&mut client_2, Action::control(&m2).serialize()).await?? {
            Response::Sync(actions::Sync::ControlQueue(queue)) => {
                assert!(queue.contains(&lm1));
                assert!(queue.contains(&lm2));
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    #[tokio::test]
    async fn control_group_then_drop_advances_queue() -> Result<()> {
        let m1 = EndpointId::mock("Mock31");
        let m2 = EndpointId::mock("Mock32");

        let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;

        let mut client_1 = connect(port).await?;
        send_receive(&mut client_1, Action::control(&m1).serialize()).await??;

        let mut client_2 = connect(port).await?;
        send_receive(&mut client_2, Action::control(&m2).serialize()).await??;

        drop(client_1);
        let response = receive(&mut client_2).await??;

        assert_granted!(response);

        Ok(())
    }

    #[tokio::test]
    async fn control_group_does_not_block_observe() -> Result<()> {
        let m1 = EndpointId::mock("Mock31");
        let m2 = EndpointId::mock("Mock32");

        let port = start_server_with_group(vec![m1.clone(), m2.clone()].into()).await;

        let mut client_1 = connect(port).await?;
        let response = send_receive(&mut client_1, Action::control(&m1).serialize()).await??;
        assert_granted!(response);

        let mut client_2 = connect(port).await?;
        let response = send_receive(&mut client_2, Action::observe(&m2).serialize()).await??;

        assert_observing!(response);

        Ok(())
    }
}
