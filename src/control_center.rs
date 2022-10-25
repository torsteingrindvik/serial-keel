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
    endpoint::{
        mock::{Mock, MockId},
        Endpoint, EndpointLabel, InternalEndpointLabel,
    },
    error::Error,
    user::User,
};

pub(crate) struct ControlCenter;

/// Actions user can ask of the control center.
/// This is a superset of the actions a user
/// can ask the server.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointLabel),
    Write((InternalEndpointLabel, SerialMessage)),

    /// Create a mocked endpoint.
    CreateMockEndpoint(MockId),

    /// Remove a mocked endpoint.
    RemoveMockEndpoint(MockId),
}

impl Action {
    pub(crate) fn from_user_action(user: &User, action: actions::Action) -> Self {
        let into_internal = |label| match label {
            EndpointLabel::Tty(tty) => InternalEndpointLabel::Tty(tty),
            EndpointLabel::Mock(name) => InternalEndpointLabel::Mock(MockId {
                user: user.clone(),
                name,
            }),
        };

        match action {
            actions::Action::Observe(endpoint_label) => {
                Self::Observe(into_internal(endpoint_label))
            }
            actions::Action::Write((endpoint_label, message)) => {
                Self::Write((into_internal(endpoint_label), message))
            }
        }
    }

    #[cfg(test)]
    fn create_mock(user: &User, name: &str) -> Self {
        Self::CreateMockEndpoint(MockId {
            user: user.clone(),
            name: name.into(),
        })
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
    ObserveThis((InternalEndpointLabel, broadcast::Receiver<SerialMessage>)),
}

#[cfg(test)]
impl ControlCenterResponse {
    pub(crate) fn try_into_observe_this(self) -> Result<broadcast::Receiver<SerialMessage>, Self> {
        if let Self::ObserveThis((_label, broadcast)) = self {
            Ok(broadcast)
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

    #[cfg(test)]
    pub(crate) async fn perform_user_action(
        &self,
        user: &User,
        action: actions::Action,
    ) -> Result<ControlCenterResponse, Error> {
        let action = Action::from_user_action(user, action);
        self.perform_action(action).await
    }
}

impl ControlCenter {
    pub(crate) fn run() -> ControlCenterHandle {
        let (outbox, mut inbox) = mpsc::unbounded_channel::<ControlCenterRequest>();

        tokio::spawn(async move {
            let mut endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>> =
                HashMap::new();

            while let Some(request) = inbox.recv().await {
                let response = match request.action {
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
                    Action::Write((label, message)) => match endpoints.get(&label) {
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
    use crate::endpoint::Tty;

    use super::*;

    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn observe_non_existing_mock_does_not_mean_it_gets_created_by_cc() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_user_action(&User::new("user"), actions::Action::observe_mock("hello"))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_label() {
        let cc = ControlCenter::run();

        let response = cc
            .perform_user_action(
                &User::new("user2"),
                actions::Action::Observe(EndpointLabel::Tty(Tty::new("/dev/tty1234"))),
            )
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn make_some_mock_endpoints() {
        let cc = ControlCenter::run();

        let user = User::new("user3");
        for endpoint in ["one", "two", "three"] {
            let response = cc
                .perform_action(Action::create_mock(&user, endpoint))
                .await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }
    }

    #[tokio::test]
    async fn cannot_double_create_mock_endpoint() {
        crate::logging::init().await;

        let cc = ControlCenter::run();

        let user = User::new("user4");

        for endpoint in ["one", "two", "three"] {
            let response = cc
                .perform_action(Action::create_mock(&user, endpoint))
                .await;
            assert!(matches!(response, Ok(ControlCenterResponse::Ok)));
        }

        let response = cc.perform_action(Action::create_mock(&user, "two")).await;
        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn can_observe_created_mock_endpoint() {
        let cc = ControlCenter::run();

        let user = User::new("user5");
        let mock_endpoint = "mock";

        let response = cc
            .perform_action(Action::create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        let response = cc
            .perform_user_action(
                &user,
                actions::Action::Observe(EndpointLabel::mock(mock_endpoint)),
            )
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
            .perform_action(Action::create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        for _ in 0..10 {
            let response = cc
                .perform_user_action(
                    &user,
                    actions::Action::Observe(EndpointLabel::mock(mock_endpoint)),
                )
                .await
                .unwrap();

            assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
        }
    }

    #[tokio::test]
    async fn cannot_write_bad_endpoint() {
        let cc = ControlCenter::run();

        let user = User::new("user7");
        let label = EndpointLabel::mock("does_not_exist");

        let msg = "Hello, mock world!";
        let response = cc
            .perform_user_action(&user, actions::Action::Write((label, msg.into())))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }

    #[tokio::test]
    async fn observe_mock_loopback() {
        let cc = ControlCenter::run();

        let user = User::new("user8");
        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_user_action(&user, actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        let msg = "Hello, mock world!";
        let response = cc
            .perform_user_action(&user, actions::Action::Write((label, msg.into())))
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::Ok));

        let observed = observer.recv().await.unwrap();

        assert_eq!(msg, &observed);
    }

    #[tokio::test]
    async fn observe_mock_loopback_several_messages() {
        let cc = ControlCenter::run();

        let user = User::new("user8");
        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_user_action(&user, actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        for n in 0..10 {
            let msg = format!("msg-{n}");
            let response = cc
                .perform_user_action(&user, actions::Action::Write((label.clone(), msg.clone())))
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

        let user = User::new("user9");
        let mock_endpoint = "mock";

        cc.perform_action(Action::create_mock(&user, mock_endpoint))
            .await
            .unwrap();

        let label = EndpointLabel::mock(mock_endpoint);
        let mut observer = cc
            .perform_user_action(&user, actions::Action::Observe(label.clone()))
            .await
            .unwrap()
            .try_into_observe_this()
            .unwrap();

        // Send a sizable amount of stuff
        for n in 0..1000 {
            let msg = format!("msg-{n}");
            let response = cc
                .perform_user_action(&user, actions::Action::Write((label.clone(), msg.clone())))
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
            .perform_user_action(&User::new(""), actions::Action::Write((label, msg.into())))
            .await;

        assert!(matches!(response, Err(Error::BadRequest(_))));
    }
}
