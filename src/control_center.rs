//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
};

use futures::{channel::mpsc, SinkExt, StreamExt};
use tokio::sync::{broadcast, oneshot, TryAcquireError};
use tracing::{debug, debug_span, info, warn};

use crate::{
    config::Config,
    endpoint::{
        Endpoint, EndpointExt, EndpointLabel, EndpointSemaphore, EndpointSemaphoreId,
        InternalEndpointLabel, OwnedEndpointSemaphore,
    },
    error::Error,
    mock::{Mock, MockId},
    serial::serial_port::{SerialMessage, SerialPortBuilder},
    user::User,
};

#[derive(Debug)]
pub(crate) struct EndpointController {
    _permit: OwnedEndpointSemaphore,

    pub(crate) endpoints: HashMap<InternalEndpointLabel, mpsc::UnboundedSender<SerialMessage>>,
}

#[derive(Debug)]
pub(crate) struct EndpointControllerQueue {
    pub(crate) queue: oneshot::Receiver<EndpointController>,
    pub(crate) endpoints: Vec<InternalEndpointLabel>,
}

/// TODO
#[derive(Debug)]
pub(crate) enum MaybeEndpointController {
    /// The endpoints were available.
    Available(EndpointController),

    /// The controller was taken.
    /// [`EndpointControllerQueue`] can be awaited to gain access.
    Busy(EndpointControllerQueue),
}

pub(crate) struct ControlCenter {
    messages: mpsc::UnboundedReceiver<ControlCenterMessage>,

    endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>>,

    // Endpoint groupings.
    // Endpoints in the same groups share controllability.
    endpoint_groups: HashSet<HashSet<InternalEndpointLabel>>,

    // A mapping of endpoint label to active observers
    observers: HashMap<InternalEndpointLabel, HashSet<User>>,

    // A mapping of endpoint label to a user with exclusive access AND queued users
    controllers: HashMap<EndpointSemaphoreId, HashSet<User>>,
}

/// Actions available to ask of the control center.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointLabel),
    Control(InternalEndpointLabel),
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Observe(l) => write!(f, "observe: {l}"),
            Action::Control(l) => write!(f, "control: {l}"),
        }
    }
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
    ControlThis(MaybeEndpointController),
    ObserveThis((InternalEndpointLabel, broadcast::Receiver<SerialMessage>)),
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterMessage>);

impl ControlCenterHandle {
    pub(crate) fn new(config: &Config) -> Self {
        let (cc_requests_tx, cc_requests_rx) = mpsc::unbounded::<ControlCenterMessage>();

        let mut control_center = ControlCenter::new(config, cc_requests_rx);

        tokio::spawn(async move { control_center.run().await });

        ControlCenterHandle(cc_requests_tx)
    }

    pub(crate) async fn inform(&mut self, information: Inform) {
        self.0
            .send(ControlCenterMessage::Inform(information))
            .await
            .expect("Send ok");
    }

    pub(crate) async fn perform_action(
        &mut self,
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
            .await
            .expect("Send ok");

        rx.await.expect("Should always make a response")
    }
}

impl ControlCenter {
    pub(crate) fn new(
        config: &Config,
        requests: mpsc::UnboundedReceiver<ControlCenterMessage>,
    ) -> Self {
        let available = if config.auto_open_serial_ports {
            let available =
                tokio_serial::available_ports().expect("Need to be able to list serial ports");
            if available.is_empty() {
                info!("No serial ports available");
            }
            available
        } else {
            vec![]
        }
        .into_iter()
        .map(|serial_port_info| serial_port_info.port_name)
        .collect::<Vec<_>>();

        // let mut endpoint_groups = HashSet::new();

        let mut endpoints: HashMap<InternalEndpointLabel, Box<dyn Endpoint + Send + Sync>> =
            HashMap::new();

        for (index, group) in config.groups.iter().enumerate() {
            let shared_semaphore = EndpointSemaphore::default();

            if group.is_mock_group() {
                let group_name = format!("MockGroup{index}");

                for label in &group.0 {
                    let endpoint_name = label.as_mock().unwrap();

                    let mock_id = MockId::new(&group_name, endpoint_name);
                    let label = InternalEndpointLabel::Mock(mock_id.clone());

                    endpoints.insert(
                        label,
                        Box::new(Mock::run_with_semaphore(mock_id, shared_semaphore.clone())),
                    );
                }
            } else {
                for label in &group.0 {
                    let tty_path = label.as_tty().unwrap();
                    let endpoint = SerialPortBuilder::new(tty_path)
                        .set_semaphore(shared_semaphore.clone())
                        .build();

                    let label = InternalEndpointLabel::Tty(tty_path.into());
                    endpoints.insert(label, Box::new(endpoint));
                }
            }
        }

        for port in &available {
            let label = InternalEndpointLabel::Tty(port.clone());
            info!("Setting up endpoint for {}", label);
            let endpoint = SerialPortBuilder::new(port).build();

            endpoints.insert(label, Box::new(endpoint));
        }

        Self {
            messages: requests,
            endpoints,
            endpoint_groups: HashSet::new(),
            observers: HashMap::new(),
            controllers: HashMap::new(),
        }
    }

    fn observe(
        &mut self,
        user: User,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        if let Some(observing_users) = self.observers.get(&label) {
            if observing_users.contains(&user) {
                return Err(Error::SuperfluousRequest(format!(
                    "User `{user}` is already observing endpoint `{}`",
                    EndpointLabel::from(label)
                )));
            }
        }
        let reply = match &label {
            InternalEndpointLabel::Tty(_) => self.observe_tty(label.clone()),
            InternalEndpointLabel::Mock(_) => self.observe_mock(label.clone()),
        };

        if reply.is_ok() {
            let observers = self.observers.entry(label).or_default();
            // It's a bug if we insert the same observer twice.
            assert!(observers.insert(user));
        }

        reply
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

    fn control_tty(
        &mut self,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        todo!();
        // match self.endpoints.get(&label) {
        //     // TODO: Change from "This" to "These", i.e. group?
        //     Some(endpoint) => Ok(ControlCenterResponse::ControlThis((
        //         label,
        //         endpoint.outbox(),
        //     ))),
        //     None => Err(Error::NoSuchEndpoint(label.to_string())),
        // }
    }

    fn endpoints_controlled_by(
        &self,
        id: &EndpointSemaphoreId,
    ) -> HashMap<InternalEndpointLabel, mpsc::UnboundedSender<SerialMessage>> {
        self.endpoints
            .iter()
            .filter_map(|(label, endpoint)| {
                if &endpoint.semaphore_id() == id {
                    Some((label.clone(), endpoint.message_sender()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn control_mock(
        &mut self,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        // This feels like an anti-pattern...
        let mock_id = match &label {
            InternalEndpointLabel::Tty(_) => unreachable!(),
            InternalEndpointLabel::Mock(id) => id.clone(),
        };

        // ...and is enforced by having `InternalEndpointLabel`
        // in `.endpoints`.
        // TODO: Change to separate: `.mock_endpoints, .tty_endpoints`.
        let endpoint = self
            .endpoints
            .entry(label)
            .or_insert_with(|| Box::new(Mock::run(mock_id)));

        // TODO: Share impl with tty
        let semaphore = endpoint.semaphore();
        let id = endpoint.semaphore_id();
        let endpoints = self.endpoints_controlled_by(&id);

        let maybe_control = match semaphore.clone().inner.try_acquire_owned() {
            Ok(permit) => MaybeEndpointController::Available(EndpointController {
                _permit: OwnedEndpointSemaphore { permit, id },
                endpoints,
            }),
            Err(TryAcquireError::NoPermits) => {
                let (permit_tx, permit_rx) = oneshot::channel();
                let permit_fut = semaphore.inner.acquire_owned();
                let task_endpoints = endpoints.clone();

                tokio::spawn(async move {
                    if let Ok(permit) = permit_fut.await {
                        if permit_tx
                            .send(EndpointController {
                                _permit: OwnedEndpointSemaphore { permit, id },
                                endpoints: task_endpoints,
                            })
                            .is_err()
                        {
                            warn!("Permit acquired but no user to receive it")
                        };
                    } else {
                        warn!("Could not get permit- endpoint closed?")
                    }
                });

                // MaybeOutbox::Busy(OutboxQueue(permit_rx))
                MaybeEndpointController::Busy(EndpointControllerQueue {
                    queue: permit_rx,
                    endpoints: endpoints.keys().cloned().collect(),
                })
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        };

        Ok(ControlCenterResponse::ControlThis(maybe_control))
    }

    fn endpoint_semaphore_id(&self, label: &InternalEndpointLabel) -> Option<EndpointSemaphoreId> {
        self.endpoints
            .get(label)
            .map(|endpoint| endpoint.semaphore_id())
    }

    fn group_members(&self, label: &InternalEndpointLabel) -> HashSet<&InternalEndpointLabel> {
        let id = self.endpoint_semaphore_id(label);

        self.endpoint_groups
            .iter()
            .flatten()
            .filter(|label| self.endpoint_semaphore_id(label) == id)
            .collect()
    }

    // Check if the semaphore id matching the label is already granted or requested by the user
    fn control_requested_or_given(&self, user: &User, label: &InternalEndpointLabel) -> bool {
        self.endpoint_semaphore_id(label)
            .and_then(|id| self.controllers.get(&id))
            .map(|users| users.contains(user))
            .unwrap_or_default()
    }

    fn control(
        &mut self,
        user: User,
        label: InternalEndpointLabel,
    ) -> Result<ControlCenterResponse, Error> {
        if self.control_requested_or_given(&user, &label) {
            let group = self.group_members(&label);

            let mut error_message =
                format!("User {user:?} is already queued or already has control over {label:?}.");

            if !group.is_empty() {
                error_message +=
                    &format!(" Note that the given endpoint implies control over: {group:?}");
            }

            return Err(Error::SuperfluousRequest(error_message));
        }

        let reply = match &label {
            InternalEndpointLabel::Tty(_) => self.control_tty(label.clone()),
            InternalEndpointLabel::Mock(_) => self.control_mock(label.clone()),
        };
        if reply.is_ok() {
            let controllers = self
                .controllers
                .entry(self.endpoint_semaphore_id(&label).unwrap())
                .or_default();

            // It's a bug if we insert the same controller twice.
            assert!(controllers.insert(user));
        }

        reply
    }

    fn handle_request(
        &mut self,
        Request {
            user,
            action,
            response,
        }: Request,
    ) {
        debug!("Got action request: `{action}` from user `{user}`");

        let reply = match action {
            Action::Control(label) => self.control(user, label),
            Action::Observe(label) => self.observe(user, label),
        };

        response
            .send(reply)
            .expect("Response receiver should not drop");
    }

    fn handle_information(&mut self, information: Inform) {
        match information {
            Inform::UserLeft(user) => {
                let _span = debug_span!("User leaving", %user).entered();

                for (endpoint, observers) in self.observers.iter_mut() {
                    if observers.remove(&user) {
                        debug!(%endpoint, "No longer observing")
                    }
                }
                for (endpoint, controllers) in self.controllers.iter_mut() {
                    if controllers.remove(&user) {
                        debug!(%endpoint, "No longer controlling / queuing for control")
                    }
                }

                let endpoint_labels = self.endpoints.keys().cloned().collect::<Vec<_>>();

                for label in endpoint_labels
                    .iter()
                    .filter(|label| matches!(label, InternalEndpointLabel::Mock(_)))
                {
                    if self
                        .observers
                        .get(label)
                        .map_or(false, |observers| observers.is_empty())
                        && self
                            .controllers
                            .get(
                                &self
                                    .endpoint_semaphore_id(label)
                                    .expect("Label originated from us"),
                            )
                            .map_or(false, |controllers| controllers.is_empty())
                    {
                        debug!(%label, "No more observers/controllers for mock (using or queued), removing");
                        assert!(self.endpoints.remove(label).is_some());
                    }
                }
            }
        }
    }

    pub(crate) async fn run(&mut self) {
        while let Some(message) = self.messages.next().await {
            match message {
                ControlCenterMessage::Request(request) => self.handle_request(request),
                ControlCenterMessage::Inform(information) => self.handle_information(information),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockId;
    use crate::user::User;

    fn cc() -> ControlCenterHandle {
        ControlCenterHandle::new(&{
            Config {
                auto_open_serial_ports: false,
                ..Default::default()
            }
        })
    }

    #[tokio::test]
    async fn observe_non_existing_mock_means_it_gets_created_by_cc() {
        let mut cc = cc();

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
        let mut cc = cc();

        let response = cc
            .perform_action(
                User::new("foo"),
                Action::Observe(InternalEndpointLabel::Tty("/dev/tty1234".into())),
            )
            .await;

        assert!(matches!(response, Err(Error::NoSuchEndpoint(_))));
    }

    #[tokio::test]
    async fn can_not_observe_mock_endpoint_several_times() {
        let mut cc = cc();

        let user = User::new("Foo");
        let mock_endpoint = "FooMock";

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
                assert!(matches!(response, Err(Error::SuperfluousRequest(_))));
            }
        }
    }
}
