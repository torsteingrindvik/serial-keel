//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::collections::HashMap;

use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::debug;

use crate::{
    endpoint::{
        mock::{Mock, MockId},
        Endpoint, InternalEndpointLabel, MaybeOutbox,
    },
    error::Error,
};

pub(crate) struct ControlCenter;

/// Actions user can ask of the control center.
/// This is a superset of the actions a user
/// can ask the server.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointLabel),

    Control(InternalEndpointLabel),

    // Write((InternalEndpointLabel, SerialMessage)),
    /// Create a mocked endpoint.
    CreateMockEndpoint(MockId),

    /// Remove a mocked endpoint.
    RemoveMockEndpoint(MockId),
}

#[derive(Debug)]
pub(crate) struct ControlCenterRequest {
    action: Action,
    response: oneshot::Sender<Result<ControlCenterResponse, Error>>,
}

#[derive(Debug)]
pub(crate) enum ControlCenterResponse {
    Ok,
    ControlThis((InternalEndpointLabel, MaybeOutbox)),
    ObserveThis((InternalEndpointLabel, broadcast::Receiver<SerialMessage>)),
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterRequest>);

impl ControlCenterHandle {
    pub(crate) async fn perform_action(
        &self,
        action: Action,
    ) -> Result<ControlCenterResponse, Error> {
        let (tx, rx) = oneshot::channel();

        self.0
            .send(ControlCenterRequest {
                action,
                response: tx,
            })
            .expect("Control center should be alive");

        rx.await.expect("Should always make a response")
    }
}

impl ControlCenter {
    pub(crate) fn run() -> ControlCenterHandle {
        let (outbox, mut inbox) = mpsc::unbounded_channel::<ControlCenterRequest>();

        tokio::spawn(async move {
            let mut endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>> =
                HashMap::new();

            while let Some(request) = inbox.recv().await {
                debug!("Got request: {request:?}");

                let response = match request.action {
                    Action::Control(label) => match endpoints.get(&label) {
                        Some(endpoint) => Ok(ControlCenterResponse::ControlThis((
                            label,
                            endpoint.outbox(),
                        ))),
                        None => match &label {
                            InternalEndpointLabel::Tty(tty) => {
                                Err(Error::BadRequest(format!("No such endpoint: {tty:?}")))
                            }
                            InternalEndpointLabel::Mock(mock) => {
                                Err(Error::BadRequest(format!("No such mock endpoint: {mock}")))
                            }
                        },
                    },
                    Action::Observe(label) => match endpoints.get(&label) {
                        Some(endpoint) => Ok(ControlCenterResponse::ObserveThis((
                            label,
                            endpoint.inbox(),
                        ))),
                        None => match &label {
                            InternalEndpointLabel::Tty(tty) => {
                                Err(Error::BadRequest(format!("No such endpoint: {tty:?}")))
                            }
                            InternalEndpointLabel::Mock(mock) => {
                                Err(Error::BadRequest(format!("No such mock endpoint: {mock}")))
                            }
                        },
                    },
                    Action::CreateMockEndpoint(mock_id) => {
                        let label = InternalEndpointLabel::Mock(mock_id.clone());
                        if endpoints.get(&label).is_some() {
                            Err(Error::BadRequest(format!(
                                "Endpoint `{label:?}` already exists"
                            )))
                        } else {
                            let endpoint = Box::new(Mock::run(mock_id));
                            assert!(endpoints.insert(label.clone(), endpoint).is_none());
                            Ok(ControlCenterResponse::Ok)
                        }
                    }
                    Action::RemoveMockEndpoint(mock_id) => {
                        let label = InternalEndpointLabel::Mock(mock_id);
                        match endpoints.remove(&label) {
                            Some(_) => Ok(ControlCenterResponse::Ok),
                            None => Err(Error::BadRequest(format!(
                                "Endpoint `{label:?}` does not exist"
                            ))),
                        }
                    }
                };

                request
                    .response
                    .send(response)
                    .expect("Response receiver should not drop");
            }
            unreachable!("ControlCenter run over not possible- we own a sender so there will always be at least one alive");
        });

        ControlCenterHandle(outbox)
    }
}

#[cfg(test)]
mod tests {
    use crate::{endpoint::Tty, user::User};

    use super::*;

    fn create_mock(user: &User, name: &str) -> Action {
        Action::CreateMockEndpoint(MockId {
            user: user.clone(),
            name: name.into(),
        })
    }

    #[tokio::test]
    async fn observe_non_existing_mock_does_not_mean_it_gets_created_by_cc() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_action(Action::Observe(InternalEndpointLabel::Mock(MockId::new(
                "user1", "mock",
            ))))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_label() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_action(Action::Observe(InternalEndpointLabel::Tty(Tty::new(
                "/dev/tty1234",
            ))))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn make_some_mock_endpoints() {
        let cc = ControlCenter::run();

        let user = User::new("user3");
        for endpoint in ["one", "two", "three"] {
            let response = cc.perform_action(create_mock(&user, endpoint)).await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }
    }

    #[tokio::test]
    async fn cannot_double_create_mock_endpoint() {
        crate::logging::init().await;

        let cc = ControlCenter::run();

        let user = User::new("user4");

        for endpoint in ["one", "two", "three"] {
            let response = cc.perform_action(create_mock(&user, endpoint)).await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }

        let response = cc.perform_action(create_mock(&user, "two")).await;
        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn can_observe_created_mock_endpoint() {
        let cc = ControlCenter::run();

        let user = User::new("user5");
        let mock_endpoint = "mock";

        let response = cc
            .perform_action(create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        let response = cc
            .perform_action(Action::Observe(InternalEndpointLabel::Mock(MockId::new(
                "user5", "mock",
            ))))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
    }

    #[tokio::test]
    async fn can_observe_created_mock_endpoint_several_times() {
        let cc = ControlCenter::run();

        let user = User::new("user6");
        let mock_endpoint = "mock";

        let response = cc
            .perform_action(create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        for _ in 0..10 {
            let response = cc
                .perform_action(Action::Observe(InternalEndpointLabel::Mock(MockId {
                    user: user.clone(),
                    name: mock_endpoint.to_owned(),
                })))
                .await
                .unwrap();

            assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
        }
    }
}
