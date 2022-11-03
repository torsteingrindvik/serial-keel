//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{collections::HashMap, fmt::Debug};

use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::debug;

use crate::{
    endpoint::{mock::Mock, Endpoint, InternalEndpointLabel, MaybeOutbox},
    error::Error,
    user::User,
};

pub(crate) struct ControlCenter {
    requests: mpsc::UnboundedReceiver<ControlCenterRequest>,
    endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>>,
    observers: HashMap<User, InternalEndpointLabel>,
}

/// Actions available to ask of the control center.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointLabel),
    Control(InternalEndpointLabel),
}

pub(crate) struct ControlCenterRequest {
    user: User,
    action: Action,
    response: oneshot::Sender<Result<ControlCenterResponse, Error>>,
}

impl Debug for ControlCenterRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlCenterRequest")
            .field("action", &self.action)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) enum ControlCenterResponse {
    ControlThis((InternalEndpointLabel, MaybeOutbox)),
    ObserveThis((InternalEndpointLabel, broadcast::Receiver<SerialMessage>)),
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterRequest>);

impl ControlCenterHandle {
    pub(crate) fn new() -> Self {
        let (cc_requests_tx, cc_requests_rx) = mpsc::unbounded_channel::<ControlCenterRequest>();

        let mut control_center = ControlCenter::new(cc_requests_rx);

        tokio::spawn(async move { control_center.run().await });

        ControlCenterHandle(cc_requests_tx)
    }

    pub(crate) async fn perform_action(
        &self,
        user: User,
        action: Action,
    ) -> Result<ControlCenterResponse, Error> {
        let (tx, rx) = oneshot::channel();

        self.0
            .send(ControlCenterRequest {
                action,
                response: tx,
                user,
            })
            .expect("Control center should be alive");

        rx.await.expect("Should always make a response")
    }
}

impl ControlCenter {
    pub(crate) fn new(requests: mpsc::UnboundedReceiver<ControlCenterRequest>) -> Self {
        Self {
            requests,
            endpoints: HashMap::new(),
            observers: HashMap::new(),
        }
    }

    fn observe(
        &mut self,
        user: User,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        if self.observers.contains_key(&user) {
            Err(Error::BadUsage(format!(
                "User {user:?} is already observing endpoint {label:?}"
            )))
        } else {
            match &label {
                InternalEndpointLabel::Tty(_) => self.observe_tty(label),
                InternalEndpointLabel::Mock(_) => self.observe_mock(label),
            }
        }
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

    pub(crate) async fn run(&mut self) {
        while let Some(ControlCenterRequest {
            user,
            action,
            response,
        }) = self.requests.recv().await
        {
            debug!("Got action request: {action:?} from user {user:?}");

            let reply = match action {
                Action::Control(label) => self.control(label),
                Action::Observe(label) => {
                    let reply = self.observe(user.clone(), label.clone());

                    if reply.is_ok() {
                        assert!(self.observers.insert(user, label).is_none());
                    }

                    reply
                } // TODO: This does not need to be an explicit action,
                  // just do it implicitly when no observers left
                  // Action::RemoveMockEndpoint(mock_id) => {
                  //     let label = InternalEndpointLabel::Mock(mock_id);
                  //     match endpoints.remove(&label) {
                  //         Some(_) => Ok(ControlCenterResponse::Ok),
                  //         None => Err(Error::NoSuchEndpoint(label.to_string())),
                  //     }
                  // }
            };

            response
                .send(reply)
                .expect("Response receiver should not drop");
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
