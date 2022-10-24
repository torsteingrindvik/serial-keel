//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::collections::HashMap;

use futures::SinkExt;
use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    actions,
    endpoint::{mock::Mock, Endpoint, EndpointLabel},
    error::Error,
};

pub(crate) struct ControlCenter {
    endpoints: HashMap<EndpointLabel, Box<dyn Endpoint + Send>>,

    inbox: mpsc::UnboundedReceiver<ControlCenterRequest>,
    outbox: mpsc::UnboundedSender<ControlCenterRequest>,
}

/// Actions user can ask of the control center.
/// This is a superset of the actions a user
/// can ask the server.
#[derive(Debug)]
pub(crate) enum Action {
    UserAction(actions::Action),

    /// Create a mocked endpoint.
    CreateMockEndpoint {
        /// The endpoint's name.
        /// After creation, it can be referred to via [`Endpoint::Mock`].
        name: String,
    },
    /// Remove a mocked endpoint.
    RemoveMockEndpoint {
        /// The endpoint's name.
        name: String,
    },
}

impl Action {
    fn create_mock(name: &str) -> Self {
        Self::CreateMockEndpoint { name: name.into() }
    }

    fn remove_mock(name: &str) -> Self {
        Self::RemoveMockEndpoint { name: name.into() }
    }
}

impl From<actions::Action> for Action {
    fn from(v: actions::Action) -> Self {
        Self::UserAction(v)
    }
}

#[derive(Debug)]
pub(crate) struct ControlCenterRequest {
    action: Action,
    response: oneshot::Sender<Result<ControlCenterResponse, Error>>,
}

#[derive(Debug)]
pub(crate) enum ControlCenterResponse {
    Ok,
    ObserveThis(broadcast::Receiver<SerialMessage>),
}

impl ControlCenterResponse {
    pub(crate) fn try_into_observe_this(self) -> Result<broadcast::Receiver<SerialMessage>, Self> {
        if let Self::ObserveThis(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterRequest>);

impl ControlCenterHandle {
    pub(crate) async fn perform_action(
        &self,
        action: impl Into<Action>,
    ) -> Result<ControlCenterResponse, Error> {
        let (tx, rx) = oneshot::channel();

        self.0
            .send(ControlCenterRequest {
                action: action.into(),
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
            let mut endpoints: HashMap<EndpointLabel, Box<dyn Endpoint + Send + Sync>> =
                HashMap::new();

            while let Some(request) = inbox.recv().await {
                let response = match request.action {
                    Action::UserAction(action) => match action {
                        actions::Action::Observe(label) => match endpoints.get(&label) {
                            Some(endpoint) => {
                                Ok(ControlCenterResponse::ObserveThis(endpoint.inbox()))
                            }
                            None => match &label {
                                EndpointLabel::Tty(tty) => {
                                    Err(Error::BadRequest(format!("No such endpoint: {tty:?}")))
                                }
                                EndpointLabel::Mock(mock) => {
                                    let endpoint = Box::new(Mock::run(mock));
                                    let inbox = endpoint.inbox();
                                    assert!(endpoints.insert(label, endpoint).is_none());
                                    Ok(ControlCenterResponse::ObserveThis(inbox))
                                }
                            },
                        },
                        actions::Action::Write((label, message)) => match endpoints.get(&label) {
                            Some(endpoint) => {
                                endpoint
                                    .outbox()
                                    .send(message)
                                    .await
                                    .expect("Endpoint receiver should be alive");
                                Ok(ControlCenterResponse::Ok)
                            }
                            None => Err(Error::BadRequest("No such endpoint".into())),
                        },
                    },
                    Action::CreateMockEndpoint { name } => {
                        let label = EndpointLabel::Mock(name.clone());
                        if endpoints.get(&label).is_some() {
                            Err(Error::BadRequest(format!(
                                "Endpoint `{label:?}` already exists"
                            )))
                        } else {
                            endpoints.insert(label, Box::new(Mock::run(&name)));
                            Ok(ControlCenterResponse::Ok)
                        }
                    }
                    Action::RemoveMockEndpoint { name } => {
                        let label = EndpointLabel::Mock(name.clone());
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
    use crate::endpoint::Tty;

    use super::*;

    use pretty_assertions::assert_eq;

    // Mock endpoints are created automatically
    #[tokio::test]
    async fn observe_non_existing_mock_label() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_action(actions::Action::Observe(EndpointLabel::Mock(
                "hello".into(),
            )))
            .await;

        assert!(matches!(
            response,
            Ok(ControlCenterResponse::ObserveThis(_))
        ));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_label() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_action(actions::Action::Observe(EndpointLabel::Tty(Tty::new(
                "/dev/tty1234",
            ))))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn make_some_mock_endpoints() {
        let cc = ControlCenter::run();

        for endpoint in ["one", "two", "three"] {
            let response = cc
                .perform_action(Action::CreateMockEndpoint {
                    name: endpoint.into(),
                })
                .await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }
    }

    #[tokio::test]
    async fn cannot_double_create_mock_endpoint() {
        let cc = ControlCenter::run();

        for endpoint in ["one", "two", "three"] {
            let response = cc
                .perform_action(Action::CreateMockEndpoint {
                    name: endpoint.into(),
                })
                .await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }

        let response = cc
            .perform_action(Action::CreateMockEndpoint {
                name: "two".into(), // already exists
            })
            .await;
        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn can_observe_created_mock_endpoint() {
        let cc = ControlCenter::run();

        let mock_endpoint = "mock";

        let response = cc
            .perform_action(Action::create_mock(mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        let response = cc
            .perform_action(actions::Action::Observe(EndpointLabel::mock(mock_endpoint)))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
    }

    #[tokio::test]
    async fn can_observe_created_mock_endpoint_several_times() {
        let cc = ControlCenter::run();

        let mock_endpoint = "mock";

        let response = cc
            .perform_action(Action::create_mock(mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        for _ in 0..10 {
            let response = cc
                .perform_action(actions::Action::Observe(EndpointLabel::mock(mock_endpoint)))
                .await
                .unwrap();

            assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
        }
    }

    #[tokio::test]
    async fn cannot_write_bad_endpoint() {
        let cc = ControlCenter::run();

        let label = EndpointLabel::mock("does_not_exist");

        let msg = "Hello, mock world!";
        let response = cc
            .perform_action(actions::Action::Write((label, msg.into())))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn observe_mock_loopback() {
        let cc = ControlCenter::run();

        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_action(actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        let msg = "Hello, mock world!";
        let response = cc
            .perform_action(actions::Action::Write((label, msg.into())))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        let observed = observer.recv().await.unwrap();

        assert_eq!(msg, &observed);
    }

    #[tokio::test]
    async fn observe_mock_loopback_several_messages() {
        let cc = ControlCenter::run();

        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_action(actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        for n in 0..10 {
            let msg = format!("msg-{n}");
            let response = cc
                .perform_action(actions::Action::Write((label.clone(), msg.clone())))
                .await
                .unwrap();

            assert!(matches!(response, ControlCenterResponse::Ok));

            let observed = observer.recv().await.unwrap();
            assert_eq!(msg, observed);
        }
    }

    #[tokio::test]
    async fn observe_mock_loopback_several_messages_queued() {
        let cc = ControlCenter::run();

        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_action(actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        // Send a sizable amount of stuff
        for n in 0..1000 {
            let msg = format!("msg-{n}");
            let response = cc
                .perform_action(actions::Action::Write((label.clone(), msg.clone())))
                .await
                .unwrap();

            assert!(matches!(response, ControlCenterResponse::Ok));
        }

        // Now receive each in order
        for n in 0..1000 {
            let msg = format!("msg-{n}");
            let observed = observer.recv().await.unwrap();
            assert_eq!(msg, observed);
        }
    }

    #[tokio::test]
    async fn cannot_remove_bad_endpoint() {
        let cc = ControlCenter::run();

        let label = EndpointLabel::mock("does_not_exist");

        let msg = "Hello, mock world!";
        let response = cc
            .perform_action(actions::Action::Write((label, msg.into())))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }
}
