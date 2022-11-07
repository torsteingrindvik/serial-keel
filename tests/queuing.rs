// TODO: Testcases!
//
// Especially:
//
// 1. Available, hold it
// 2. Busy, get queued
// 3. Busy, also queed
// 4. First in queue drops
// 5. Semaphore dropped
// 6. Check that first in queue is ignored, second in place gets it
//
// Also check things like if there was a queue, but all queuers dropped, _then_ someone arrives.
// And so on.

// use serial_keel::{
//     actions::{Action, Response},
//     endpoint::EndpointLabel,
// };
// use tracing::info;
// use common::{connect, receive, send_receive};

// TODO: Three users, check queue

mod common;

// Feature: Can't test queuing if endpoints are not shared
#[cfg(feature = "mocks-share-endpoints")]
mod queuing {
    use std::time::Duration;

    use super::common::*;
    use color_eyre::Result;
    use pretty_assertions::assert_eq;
    use serial_keel::{
        actions::{Action, Response},
        endpoint::EndpointLabel,
    };

    #[tokio::test]
    async fn second_user_is_queued() -> Result<()> {
        // Shared data
        let label = EndpointLabel::Mock("queue".into());
        let request = Action::control(&label).serialize();

        let port = start_server().await;

        // Client 1
        let mut client_1 = connect(port).await?;
        let response = send_receive(&mut client_1, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlGranted(label_clone), response);

        // Client 2
        let mut client_2 = connect(port).await?;
        let response = send_receive(&mut client_2, request).await??;

        assert_eq!(Response::ControlQueue(label), response);

        Ok(())
    }

    #[tokio::test]
    async fn second_user_gets_access_after_first_user_leaves() -> Result<()> {
        // Shared data
        let label = EndpointLabel::Mock("queue-then-leave".into());
        let request = Action::control(&label).serialize();

        let port = start_server().await;

        // Client 1
        let mut client_1 = connect(port).await?;
        let response = send_receive(&mut client_1, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlGranted(label_clone), response);

        // Client 2
        let mut client_2 = connect(port).await?;
        let response = send_receive(&mut client_2, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlQueue(label_clone), response);

        // Client 1 leaves
        drop(client_1);

        let response = receive(&mut client_2).await??;
        assert_eq!(Response::ControlGranted(label), response);

        // This is just to observe in logs that mocks are removed after the
        // last observer leaves.
        drop(client_2);
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(())
    }

    #[tokio::test]
    async fn three_users_control_granted_in_order() -> Result<()> {
        // Shared data
        let label = EndpointLabel::Mock("queue-three-users".into());
        let request = Action::control(&label).serialize();

        let port = start_server().await;

        // Client 1
        let mut client_1 = connect(port).await?;
        let response = send_receive(&mut client_1, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlGranted(label_clone), response);

        // Client 2
        let mut client_2 = connect(port).await?;
        let response = send_receive(&mut client_2, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlQueue(label_clone), response);

        // Client 3
        let mut client_3 = connect(port).await?;
        let response = send_receive(&mut client_3, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlQueue(label_clone), response);

        // Client 1 leaves
        drop(client_1);

        // Client 2 should get first
        let response = receive(&mut client_2).await??;
        let label_clone = label.clone();
        assert_eq!(Response::ControlGranted(label_clone), response);

        // Client 2 leaves
        drop(client_2);

        // Client 3 should now get access
        let response = receive(&mut client_3).await??;
        assert_eq!(Response::ControlGranted(label), response);

        Ok(())
    }

    #[tokio::test]
    async fn three_users_but_queued_user_leaves() -> Result<()> {
        // Shared data
        let label = EndpointLabel::Mock("queue-three-users".into());
        let request = Action::control(&label).serialize();

        let port = start_server().await;

        // Client 1
        let mut client_1 = connect(port).await?;
        let response = send_receive(&mut client_1, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlGranted(label_clone), response);

        // Client 2
        let mut client_2 = connect(port).await?;
        let response = send_receive(&mut client_2, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlQueue(label_clone), response);

        // Client 3
        let mut client_3 = connect(port).await?;
        let response = send_receive(&mut client_3, request.clone()).await??;

        let label_clone = label.clone();
        assert_eq!(Response::ControlQueue(label_clone), response);

        // Client 2 leaves while in queue
        drop(client_2);

        // Client 1 leaves
        drop(client_1);

        // Client 3 should now get access
        let response = receive(&mut client_3).await??;
        assert_eq!(Response::ControlGranted(label), response);

        Ok(())
    }
}
