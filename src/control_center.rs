//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
};

use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, warn};

use crate::{
    endpoint::{mock::Mock, Endpoint, InternalEndpointLabel, MaybeOutbox},
    error::Error,
    user::User,
};

pub(crate) struct ControlCenter {
    messages: mpsc::UnboundedReceiver<ControlCenterMessage>,

    endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>>,
    observers: HashMap<InternalEndpointLabel, HashSet<User>>,
}

/// Actions available to ask of the control center.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointLabel),
    Control(InternalEndpointLabel),
}

/// Inform the control center of events.
#[derive(Debug)]
pub(crate) enum Inform {
    /// A user left.
    /// This is important to know because we might need to clean up state after them.
    UserLeft(User),
}

pub(crate) struct Request {
    user: User,
    action: Action,
    response: oneshot::Sender<Result<ControlCenterResponse, Error>>,
}

pub(crate) enum ControlCenterMessage {
    Request(Request),
    Inform(Inform),
}

impl Debug for ControlCenterMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlCenterMessage::Request(request) => f
                .debug_struct("ControlCenterMessage")
                .field("user", &request.user)
                .field("action", &request.action)
                .finish(),
            ControlCenterMessage::Inform(i) => f
                .debug_struct("ControlCenterMessage")
                .field("information", &i)
                .finish(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum ControlCenterResponse {
    ControlThis((InternalEndpointLabel, MaybeOutbox)),
    ObserveThis((InternalEndpointLabel, broadcast::Receiver<SerialMessage>)),
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterMessage>);

impl ControlCenterHandle {
    pub(crate) fn new() -> Self {
        let (cc_requests_tx, cc_requests_rx) = mpsc::unbounded_channel::<ControlCenterMessage>();

        let mut control_center = ControlCenter::new(cc_requests_rx);

        tokio::spawn(async move { control_center.run().await });

        ControlCenterHandle(cc_requests_tx)
    }
    pub(crate) fn inform(&self, information: Inform) {
        self.0
            .send(ControlCenterMessage::Inform(information))
            .expect("Control center should be alive");
    }

    pub(crate) async fn perform_action(
        &self,
        user: User,
        action: Action,
    ) -> Result<ControlCenterResponse, Error> {
        let (tx, rx) = oneshot::channel();

        self.0
            .send(ControlCenterMessage::Request(Request {
                action,
                response: tx,
                user,
            }))
            .expect("Control center should be alive");

        rx.await.expect("Should always make a response")
    }
}

impl ControlCenter {
    pub(crate) fn new(requests: mpsc::UnboundedReceiver<ControlCenterMessage>) -> Self {
        Self {
            messages: requests,
            endpoints: HashMap::new(),
            observers: HashMap::new(),
        }
    }

    fn observe(
        &mut self,
        user: User,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        if let Some(observing_users) = self.observers.get(&label) {
            if observing_users.contains(&user) {
                return Err(Error::BadUsage(format!(
                    "User {user:?} is already observing endpoint {label:?}"
                )));
            }
        }
        match &label {
            InternalEndpointLabel::Tty(_) => self.observe_tty(label),
            InternalEndpointLabel::Mock(_) => self.observe_mock(label),
        }

        // if self.observers.get(&label).is_some_and(|observing_users| observing_users.contains(&user)) {

        // }

        // if self.observers.contains_key(&user) {
        //     Err(Error::BadUsage(format!(
        //         "User {user:?} is already observing endpoint {label:?}"
        //     )))
        // } else {
        // }
    }

    fn observe_mock(
        &mut self,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        let mock_id = match &label {
            InternalEndpointLabel::Tty(_) => unreachable!(),
            InternalEndpointLabel::Mock(id) => id.clone(),
        };

        let endpoint = self
            .endpoints
            .entry(label.clone())
            .or_insert_with(|| Box::new(Mock::run(mock_id)));

        Ok(ControlCenterResponse::ObserveThis((
            label,
            endpoint.inbox(),
        )))
    }

    fn observe_tty(
        &mut self,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        match self.endpoints.get(&label) {
            Some(endpoint) => Ok(ControlCenterResponse::ObserveThis((
                label,
                endpoint.inbox(),
            ))),
            None => Err(Error::NoSuchEndpoint(label.to_string())),
        }
    }

    fn control(&mut self, label: InternalEndpointLabel) -> Result<ControlCenterResponse, Error> {
        // TODO: Check if already controlling.
        // Or should we? How

        match self.endpoints.get(&label) {
            Some(endpoint) => Ok(ControlCenterResponse::ControlThis((
                label,
                endpoint.outbox(),
            ))),
            None => match &label {
                InternalEndpointLabel::Tty(tty) => Err(Error::NoSuchEndpoint(tty.to_string())),
                InternalEndpointLabel::Mock(mock) => Err(Error::NoSuchEndpoint(mock.to_string())),
            },
        }
    }

    fn handle_request(
        &mut self,
        Request {
            user,
            action,
            response,
        }: Request,
    ) {
        debug!("Got action request: {action:?} from user {user:?}");

        let reply = match action {
            Action::Control(label) => self.control(label),
            Action::Observe(label) => {
                let reply = self.observe(user.clone(), label.clone());

                if reply.is_ok() {
                    let observers = self.observers.entry(label).or_default();
                    // It's a bug if we insert the same observer twice.
                    assert!(observers.insert(user));
                }

                reply
            }
        };

        response
            .send(reply)
            .expect("Response receiver should not drop");
    }

    fn handle_information(&mut self, information: Inform) {
        match information {
            Inform::UserLeft(user) => {
                for (endpoint, observers) in self.observers.iter_mut() {
                    debug!("User {user:?} no longer observing {endpoint:?}");
                    observers.remove(&user);

                    if observers.is_empty() && matches!(endpoint, &InternalEndpointLabel::Mock(_)) {
                        debug!("No more users observing mock endpoint {endpoint:?}, removing");
                        if self.endpoints.remove(endpoint).is_none() {
                            warn!("Endpoint {endpoint} could was not in endpoints- bug?")
                        }
                    }
                }
            }
        }
    }

    pub(crate) async fn run(&mut self) {
        while let Some(message) = self.messages.recv().await {
            match message {
                ControlCenterMessage::Request(request) => self.handle_request(request),
                ControlCenterMessage::Inform(information) => self.handle_information(information),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoint::mock::MockId;
    use crate::{endpoint::Tty, user::User};

    use super::*;

    #[tokio::test]
    async fn observe_non_existing_mock_means_it_gets_created_by_cc() {
        let cc = ControlCenterHandle::new();

        let response = cc
            .perform_action(
                User::new("foo"),
                Action::Observe(InternalEndpointLabel::Mock(MockId::new("user1", "mock"))),
            )
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_label() {
        let cc = ControlCenterHandle::new();

        let response = cc
            .perform_action(
                User::new("foo"),
                Action::Observe(InternalEndpointLabel::Tty(Tty::new("/dev/tty1234"))),
            )
            .await;

        assert!(matches!(response, Err(Error::NoSuchEndpoint(_))));
    }

    #[tokio::test]
    async fn can_not_observe_mock_endpoint_several_times() {
        let cc = ControlCenterHandle::new();

        let user = User::new("foo");
        let mock_endpoint = "mock";

        for i in 0..10 {
            let response = cc
                .perform_action(
                    user.clone(),
                    Action::Observe(InternalEndpointLabel::Mock(MockId {
                        user: user.clone(),
                        name: mock_endpoint.to_owned(),
                    })),
                )
                .await;

            if i == 0 {
                assert!(matches!(
                    response,
                    Ok(ControlCenterResponse::ObserveThis(_))
                ));
            } else {
                assert!(matches!(response, Err(Error::BadUsage(_))));
            }
        }
    }
}
