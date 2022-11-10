//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
};

use futures::{channel::mpsc, SinkExt, StreamExt};
use itertools::{Either, Itertools};
use tokio::sync::{broadcast, oneshot, OwnedSemaphorePermit, TryAcquireError};
use tracing::{debug, debug_span, info, warn};

use crate::{
    config::{Config, ConfigEndpoint},
    endpoint::{
        Endpoint, EndpointExt, EndpointId, EndpointSemaphore, EndpointSemaphoreId,
        InternalEndpointId, Label,
    },
    error::Error,
    mock::{MockBuilder, MockId},
    serial::serial_port::{SerialMessage, SerialPortBuilder},
    user::User,
};

#[derive(Debug)]
pub(crate) struct EndpointController {
    _permit: OwnedSemaphorePermit,

    pub(crate) endpoints: HashMap<InternalEndpointId, mpsc::UnboundedSender<SerialMessage>>,
}

#[derive(Debug)]
pub(crate) struct EndpointControllerQueue {
    pub(crate) inner: oneshot::Receiver<EndpointController>,
    pub(crate) endpoints: Vec<InternalEndpointId>,
}

/// TODO
#[derive(Debug)]
pub(crate) enum MaybeEndpointController {
    /// The endpoints were available.
    Available(EndpointController),

    /// The controller(s) was/were taken.
    /// The queues [`EndpointControllerQueue`] can be awaited to gain access.
    Busy(EndpointControllerQueue),
    // TODO: Busy(LabelQueue)?
}

// impl MaybeEndpointController {
//     pub(crate) fn try_into_available(self) -> Result<EndpointController, Self> {
//         if let Self::Available(v) = self {
//             Ok(v)
//         } else {
//             Err(self)
//         }
//     }

//     pub(crate) fn try_into_busy(self) -> Result<EndpointControllerQueue, Self> {
//         if let Self::Busy(v) = self {
//             Ok(v)
//         } else {
//             Err(self)
//         }
//     }
// }

pub(crate) struct ControlCenter {
    messages: mpsc::UnboundedReceiver<ControlCenterMessage>,

    endpoints: HashMap<InternalEndpointId, Box<dyn Endpoint + Send + Sync>>,

    // Endpoint groupings.
    // Endpoints in the same groups share controllability.
    // endpoint_groups: HashSet<HashSet<InternalEndpointId>>,

    // A mapping of endpoint id to active observers
    observers: HashMap<InternalEndpointId, HashSet<User>>,

    // A mapping of endpoint id to a user with exclusive access AND queued users
    controllers: HashMap<EndpointSemaphoreId, HashSet<User>>,
}

/// Actions available to ask of the control center.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointId),
    Control(InternalEndpointId),
    ControlAny(Label),
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Observe(l) => write!(f, "observe: {l}"),
            Action::Control(l) => write!(f, "control: {l}"),
            Action::ControlAny(l) => write!(f, "control any: {l}"),
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
    ObserveThis((InternalEndpointId, broadcast::Receiver<SerialMessage>)),
}

#[derive(Debug, Clone)]
pub(crate) struct ControlCenterHandle(mpsc::UnboundedSender<ControlCenterMessage>);

impl ControlCenterHandle {
    pub(crate) fn new(config: &Config) -> Self {
        let (cc_requests_tx, cc_requests_rx) = mpsc::unbounded::<ControlCenterMessage>();

        let mut control_center = ControlCenter::new(config.clone(), cc_requests_rx);

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
        config: Config,
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

        let mut endpoints: HashMap<InternalEndpointId, Box<dyn Endpoint + Send + Sync>> =
            HashMap::new();

        for ConfigEndpoint { endpoint_id, label } in config.endpoints {
            match endpoint_id {
                EndpointId::Tty(tty) => {
                    let mut builder = SerialPortBuilder::new(&tty);

                    if let Some(label) = label {
                        builder = builder.add_label(label);
                    }

                    let id = InternalEndpointId::Tty(tty);
                    endpoints.insert(id, Box::new(builder.build()));
                }
                EndpointId::Mock(mock) => {
                    let mock_id = MockId::new("MockFromConfig", &mock);
                    let id = InternalEndpointId::Mock(mock_id.clone());

                    let mut builder = MockBuilder::new(mock_id);
                    if let Some(label) = label {
                        builder = builder.add_label(label);
                    }

                    endpoints.insert(id, Box::new(builder.build()));
                }
            }
        }

        for (index, group) in config.groups.iter().enumerate() {
            let shared_semaphore = EndpointSemaphore::default();

            let group_label = &group.label;

            if group.is_mock_group() {
                let group_name = format!("MockGroup{index}");

                for id in &group.endpoint_ids {
                    let endpoint_name = id.as_mock().unwrap();

                    let mock_id = MockId::new(&group_name, endpoint_name);
                    let id = InternalEndpointId::Mock(mock_id.clone());

                    let mut builder =
                        MockBuilder::new(mock_id).set_semaphore(shared_semaphore.clone());
                    if let Some(label) = &group_label {
                        builder = builder.add_label(label.clone());
                    }

                    endpoints.insert(id, Box::new(builder.build()));
                }
            } else {
                for id in &group.endpoint_ids {
                    let tty_path = id.as_tty().unwrap();
                    let mut builder =
                        SerialPortBuilder::new(tty_path).set_semaphore(shared_semaphore.clone());

                    if let Some(label) = &group_label {
                        builder = builder.add_label(label.clone());
                    }

                    let id = InternalEndpointId::Tty(tty_path.into());
                    endpoints.insert(id, Box::new(builder.build()));
                }
            }
        }

        for port in &available {
            let id = InternalEndpointId::Tty(port.clone());
            info!("Setting up endpoint for {}", id);
            let endpoint = SerialPortBuilder::new(port).build();

            endpoints.insert(id, Box::new(endpoint));
        }

        Self {
            messages: requests,
            endpoints,
            // endpoint_groups: HashSet::new(),
            observers: HashMap::new(),
            controllers: HashMap::new(),
        }
    }

    fn observe(
        &mut self,
        user: User,
        id: InternalEndpointId,
    ) -> Result<ControlCenterResponse, Error> {
        if let Some(observing_users) = self.observers.get(&id) {
            if observing_users.contains(&user) {
                return Err(Error::SuperfluousRequest(format!(
                    "User `{user}` is already observing endpoint `{}`",
                    EndpointId::from(id)
                )));
            }
        }
        let reply = match &id {
            InternalEndpointId::Tty(_) => self.observe_tty(id.clone()),
            InternalEndpointId::Mock(_) => self.observe_mock(id.clone()),
        };

        if reply.is_ok() {
            let observers = self.observers.entry(id).or_default();
            // It's a bug if we insert the same observer twice.
            assert!(observers.insert(user));
        }

        reply
    }

    fn observe_mock(&mut self, id: InternalEndpointId) -> Result<ControlCenterResponse, Error> {
        let mock_id = match &id {
            InternalEndpointId::Tty(_) => unreachable!(),
            InternalEndpointId::Mock(id) => id.clone(),
        };

        let endpoint = self
            .endpoints
            .entry(id.clone())
            // .or_insert_with(|| Box::new(MockHandle::run(mock_id)));
            .or_insert_with(|| Box::new(MockBuilder::new(mock_id).build()));

        Ok(ControlCenterResponse::ObserveThis((id, endpoint.inbox())))
    }

    fn observe_tty(&mut self, id: InternalEndpointId) -> Result<ControlCenterResponse, Error> {
        match self.endpoints.get(&id) {
            Some(endpoint) => Ok(ControlCenterResponse::ObserveThis((id, endpoint.inbox()))),
            None => Err(Error::NoSuchEndpoint(id.to_string())),
        }
    }

    fn control_tty(&mut self, _id: InternalEndpointId) -> Result<MaybeEndpointController, Error> {
        todo!();
        // match self.endpoints.get(&id) {
        //     // TODO: Change from "This" to "These", i.e. group?
        //     Some(endpoint) => Ok(ControlCenterResponse::ControlThis((
        //         id,
        //         endpoint.outbox(),
        //     ))),
        //     None => Err(Error::NoSuchEndpoint(id.to_string())),
        // }
    }

    fn endpoints_controlled_by(
        &self,
        id: &EndpointSemaphoreId,
    ) -> HashMap<InternalEndpointId, mpsc::UnboundedSender<SerialMessage>> {
        self.endpoints
            .iter()
            .filter_map(|(id_, endpoint)| {
                if &endpoint.semaphore_id() == id {
                    Some((id_.clone(), endpoint.message_sender()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn control_mock(&mut self, id: InternalEndpointId) -> Result<MaybeEndpointController, Error> {
        // This feels like an anti-pattern...
        let mock_id = match &id {
            InternalEndpointId::Tty(_) => unreachable!(),
            InternalEndpointId::Mock(id) => id.clone(),
        };

        // ...and is enforced by having `InternalEndpointid`
        // in `.endpoints`.
        // TODO: Change to separate: `.mock_endpoints, .tty_endpoints`.
        let endpoint = self
            .endpoints
            .entry(id)
            .or_insert_with(|| Box::new(MockBuilder::new(mock_id).build()));
        // .or_insert_with(|| Box::new(MockHandle::run(mock_id)));

        // TODO: Share impl with tty
        let semaphore = endpoint.semaphore();
        let id = endpoint.semaphore_id();
        let endpoints = self.endpoints_controlled_by(&id);

        let maybe_control = match semaphore.clone().inner.try_acquire_owned() {
            Ok(permit) => MaybeEndpointController::Available(EndpointController {
                _permit: permit,
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
                                _permit: permit,
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

                MaybeEndpointController::Busy(EndpointControllerQueue {
                    inner: permit_rx,
                    endpoints: endpoints.keys().cloned().collect(),
                })
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        };

        Ok(maybe_control)
    }

    fn endpoint_semaphore_id(&self, id: &InternalEndpointId) -> Option<EndpointSemaphoreId> {
        self.endpoints
            .get(id)
            .map(|endpoint| endpoint.semaphore_id())
    }

    // fn group_members(&self, id: &InternalEndpointId) -> HashSet<&InternalEndpointId> {
    //     let id_to_match = self.endpoint_semaphore_id(id);

    //     self.endpoint_groups
    //         .iter()
    //         .flatten()
    //         .filter(|id| self.endpoint_semaphore_id(id) == id_to_match)
    //         .collect()
    // }

    // Check if the semaphore id matching the id is already granted or requested by the user
    fn control_requested_or_given(&self, user: &User, id: &InternalEndpointId) -> bool {
        self.endpoint_semaphore_id(id)
            .and_then(|id| self.controllers.get(&id))
            .map(|users| users.contains(user))
            .unwrap_or_default()
    }

    fn control(
        &mut self,
        user: &User,
        id: InternalEndpointId,
    ) -> Result<MaybeEndpointController, Error> {
        if self.control_requested_or_given(user, &id) {
            // let group = self.group_members(&id);

            let error_message =
                format!("User {user:?} is already queued or already has control over {id:?}.");

            // if !group.is_empty() {
            //     error_message +=
            //         &format!(" Note that the given endpoint implies control over: {group:?}");
            // }

            return Err(Error::SuperfluousRequest(error_message));
        }

        let reply = match &id {
            InternalEndpointId::Tty(_) => self.control_tty(id.clone()),
            InternalEndpointId::Mock(_) => self.control_mock(id.clone()),
        };

        // TODO: Can be problematic if control is called for many endpoints
        if reply.is_ok() {
            let controllers = self
                .controllers
                .entry(self.endpoint_semaphore_id(&id).unwrap())
                .or_default();

            // It's a bug if we insert the same controller twice.
            assert!(controllers.insert(user.clone()));
        }

        reply
    }

    // Get endpoints matching the given label.
    // The endpoints will be unique in terms of which semaphore they
    // require in order to be controlled.
    // This is done because wanting to control a labelled endpoint within
    // a group implies control over the rest of that group,
    // so there is no need to queue for more than one within this group.
    fn labels_to_endpoint_ids(&self, label: &Label) -> Vec<InternalEndpointId> {
        self.endpoints
            .iter()
            .filter_map(|(id, endpoint)| endpoint.labels().map(|labels| (id, labels)))
            .filter(|(_, labels)| labels.contains(label))
            .map(|(id, _)| id)
            .unique_by(|id| self.endpoint_semaphore_id(id).expect("Endpoint exists"))
            .cloned()
            .collect()
    }

    fn control_any(&mut self, user: User, label: Label) -> Result<MaybeEndpointController, Error> {
        // Here's the "algorithm" for controlling any matching endpoint.
        //
        //  1.  Get list of endpoints matching the label
        //  2.  If empty, quit
        //  3.  Attempt controlling all matching ones
        //  4.  If only errors, quit
        //  5.  If at least one is available without a queue, use the first one, quit
        //  6.  Else: Make a queue which yields the first one.

        let ids = self.labels_to_endpoint_ids(&label);
        if ids.is_empty() {
            return Err(Error::NoMatchingEndpoints(label));
        }

        let (oks, errs): (Vec<_>, Vec<_>) = ids
            .clone()
            .into_iter()
            .map(|id| self.control(&user, id))
            .partition_result();

        if oks.is_empty() {
            return Err(Error::BadUsage(format!(
                "All matching endpoints: {ids:?} resulted in errors: {errs:?}"
            )));
        }

        let (available, busy): (Vec<_>, Vec<_>) =
            oks.into_iter()
                .partition_map(|maybe_contoller| match maybe_contoller {
                    MaybeEndpointController::Available(available) => Either::Left(available),
                    MaybeEndpointController::Busy(busy) => Either::Right(busy),
                });
        if let Some(controller) = available.into_iter().next() {
            Ok(MaybeEndpointController::Available(controller))
        } else {
            assert!(!busy.is_empty());

            let queues = busy
                .into_iter()
                .map(|queue| queue.inner)
                .collect::<Vec<_>>();

            let queue_fut = futures::future::select_ok(queues);

            let (controller_tx, controller_rx) = oneshot::channel();

            tokio::spawn(async move {
                let (controller, _other_futs) = queue_fut
                    .await
                    .expect("We currently cannot handle a multi-queue failing");

                if let Err(e) = controller_tx.send(controller) {
                    warn!(?e, "User left while in a label queue");
                }
            });

            Ok(MaybeEndpointController::Busy(EndpointControllerQueue {
                inner: controller_rx,
                endpoints: vec![],
            }))
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
        debug!("Got action request: `{action}` from user `{user}`");

        let reply = match action {
            Action::Observe(id) => self.observe(user, id),
            Action::Control(id) => self
                .control(&user, id)
                .map(ControlCenterResponse::ControlThis),
            Action::ControlAny(label) => self
                .control_any(user, label)
                .map(ControlCenterResponse::ControlThis),
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

                let endpoint_ids = self.endpoints.keys().cloned().collect::<Vec<_>>();

                for id in endpoint_ids
                    .iter()
                    .filter(|id| matches!(id, InternalEndpointId::Mock(_)))
                {
                    if self
                        .observers
                        .get(id)
                        .map_or(false, |observers| observers.is_empty())
                        && self
                            .controllers
                            .get(
                                &self
                                    .endpoint_semaphore_id(id)
                                    .expect("id originated from us"),
                            )
                            .map_or(false, |controllers| controllers.is_empty())
                    {
                        debug!(%id, "No more observers/controllers for mock (using or queued), removing");
                        assert!(self.endpoints.remove(id).is_some());
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
                Action::Observe(InternalEndpointId::Mock(MockId::new("user1", "mock"))),
            )
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_id() {
        let mut cc = cc();

        let response = cc
            .perform_action(
                User::new("foo"),
                Action::Observe(InternalEndpointId::Tty("/dev/tty1234".into())),
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
                    Action::Observe(InternalEndpointId::Mock(MockId {
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
