//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::{Debug, Display},
};

use futures::{channel::mpsc, SinkExt, StreamExt};
use itertools::{Either, Itertools};
use tokio::sync::{broadcast, oneshot, OwnedSemaphorePermit, TryAcquireError};
use tracing::{debug, debug_span, info, info_span, warn};

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
    // id: EndpointSemaphoreId,
    pub(crate) endpoints: HashMap<InternalEndpointId, mpsc::UnboundedSender<SerialMessage>>,
}

// impl EndpointController {
//     fn endpoint_ids(&self) -> Vec<InternalEndpointId> {
//         self.endpoints.keys().cloned().collect()
//     }
// }

#[derive(Debug)]
pub(crate) struct EndpointControllerQueue {
    pub(crate) inner: oneshot::Receiver<EndpointController>,
    pub(crate) endpoints: Vec<InternalEndpointId>,
}

impl EndpointControllerQueue {
    fn endpoint_ids(&self) -> Vec<InternalEndpointId> {
        self.endpoints.clone()
    }
}

/// TODO
#[derive(Debug)]
pub(crate) enum AvailableOrBusyEndpointController {
    /// The endpoints were available.
    Available(EndpointController),

    /// The controller(s) was/were taken.
    /// The queues [`EndpointControllerQueue`] can be awaited to gain access.
    Busy(EndpointControllerQueue),
    // TODO: Busy(LabelQueue)?
}

/// Did the user request access to
/// a specific endpoint, or a label
#[derive(Debug, Clone)]
pub(crate) enum UserRequest {
    EndpointId(EndpointId),
    Label(Label),
}

/// The context of getting access to controlling
/// something.
#[derive(Debug, Clone)]
pub(crate) struct ControlContext {
    /// The originating request.
    user_request: UserRequest,

    /// When the request resolves,
    /// which endpoints were gained control over.
    pub(crate) got_control: Option<Vec<InternalEndpointId>>,
}

impl Display for ControlContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.user_request {
            UserRequest::EndpointId(id) => write!(f, "{id}"),
            UserRequest::Label(label) => write!(f, "{label}"),
        }
    }
}

/// A controller for an endpoint, and the context of requesting that.
#[derive(Debug)]
pub(crate) struct MaybeEndpointController {
    pub(crate) context: ControlContext,
    pub(crate) inner: AvailableOrBusyEndpointController,
}

impl MaybeEndpointController {
    fn available(context: ControlContext, controller: EndpointController) -> Self {
        Self {
            context,
            inner: AvailableOrBusyEndpointController::Available(controller),
        }
    }

    fn busy(context: ControlContext, queue: EndpointControllerQueue) -> Self {
        Self {
            context,
            inner: AvailableOrBusyEndpointController::Busy(queue),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UserEvent {
    #[allow(dead_code)]
    user: User,

    #[allow(dead_code)]
    event: Event,
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Connected,
    Left,

    Observing(Vec<InternalEndpointId>),
    NoLongerObserving(Vec<InternalEndpointId>),

    InQueueFor(Vec<InternalEndpointId>),
    InControlOf(Vec<InternalEndpointId>),

    NoLongerInQueueOf(Vec<InternalEndpointId>),
    NoLongerInControlOf(Vec<InternalEndpointId>),
}

#[derive(Debug, Default)]
struct UserState {
    observing: HashSet<InternalEndpointId>,
    in_queue_of: HashSet<InternalEndpointId>,
    in_control_of: HashSet<EndpointSemaphoreId>,
}

#[derive(Debug)]
struct Events {
    // TODO: Timestamp
    log: VecDeque<UserEvent>,

    tx: broadcast::Sender<UserEvent>,
    #[allow(dead_code)]
    rx: broadcast::Receiver<UserEvent>,
}

impl Events {
    fn new() -> Self {
        let (tx, rx) = broadcast::channel(10);
        Self {
            tx,
            rx,
            log: VecDeque::new(),
        }
    }

    fn send_event(&mut self, event: UserEvent) {
        self.log.push_front(event.clone());

        // Keep a log of at most this number recent events.
        // Truncate removes from the back, which means older events are split off first.
        self.log.truncate(1000);

        self.tx.send(event).expect("Broadcast should work");
    }
}

pub(crate) struct ControlCenter {
    messages: mpsc::UnboundedReceiver<ControlCenterMessage>,

    events: Events,

    endpoints: HashMap<InternalEndpointId, Box<dyn Endpoint + Send + Sync>>,

    // A mapping of endpoint id to active observers
    // observers: HashMap<InternalEndpointId, HashSet<User>>,

    // A mapping of endpoint id to a user with exclusive access AND queued users
    // controllers: HashMap<EndpointSemaphoreId, HashSet<User>>,
    user_state: HashMap<User, UserState>,
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
    /// A user arrived.
    UserArrived(User),

    /// A user left.
    /// This is important to know because we might need to clean up state after them.
    UserLeft(User),

    NowControlling {
        user: User,
        context: ControlContext,
    },
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

impl ControlCenterResponse {
    pub(crate) fn try_into_control_this(self) -> Result<MaybeEndpointController, Self> {
        if let Self::ControlThis(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
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

    /// Inform the control center of some event.
    pub(crate) fn inform(&self, information: Inform) {
        self.0
            .unbounded_send(ControlCenterMessage::Inform(information))
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
            // observers: HashMap::new(),
            // controllers: HashMap::new(),
            events: Events::new(),
            user_state: HashMap::new(),
        }
    }

    fn is_observing(&self, user: &User, id: &InternalEndpointId) -> bool {
        self.user_state
            .get(user)
            .expect("We should know about live users")
            .observing
            .contains(id)
    }

    fn set_observing(&mut self, user: &User, id: InternalEndpointId) {
        // Assert: Just making sure we don't double insert,
        // which would be a bug on our part.
        assert!(self.user_state_mut(user).observing.insert(id.clone()));

        self.events.send_event(UserEvent {
            user: user.clone(),
            event: Event::Observing(vec![id]),
        });
    }

    fn user_state_mut(&mut self, user: &User) -> &mut UserState {
        self.user_state.get_mut(user).expect("User should be alive")
    }

    fn user_state(&self, user: &User) -> &UserState {
        self.user_state.get(user).expect("User should be alive")
    }

    fn set_controls(&mut self, user: &User, endpoints_ids: Vec<InternalEndpointId>) {
        info_span!("Now controls", %user);

        self.events.send_event(UserEvent {
            user: user.clone(),
            event: Event::InControlOf(endpoints_ids.clone()),
        });

        let mut semaphore_ids = endpoints_ids
            .into_iter()
            .map(|id| self.endpoint_semaphore_id(&id).expect("Exists"))
            .collect::<Vec<_>>();
        semaphore_ids.dedup();
        debug!("Semaphore ids: {semaphore_ids:?}");

        assert!(semaphore_ids.len() == 1);

        // Assert: Just making sure we don't double insert,
        // which would be a bug on our part.
        assert!(self
            .user_state_mut(user)
            .in_control_of
            .insert(semaphore_ids[0].clone()));
    }

    fn set_in_control_queue(&mut self, user: &User, controller_queue: &EndpointControllerQueue) {
        let endpoint_ids = controller_queue.endpoint_ids();

        self.events.send_event(UserEvent {
            user: user.clone(),
            event: Event::InQueueFor(endpoint_ids.clone()),
        });

        for id in endpoint_ids {
            assert!(self.user_state_mut(user).in_queue_of.insert(id));
        }
    }

    fn observe(
        &mut self,
        user: User,
        id: InternalEndpointId,
    ) -> Result<ControlCenterResponse, Error> {
        if self.is_observing(&user, &id) {
            return Err(Error::SuperfluousRequest(format!(
                "`{user}` is already observing endpoint `{}`",
                EndpointId::from(id)
            )));
        }

        let reply = match &id {
            InternalEndpointId::Tty(_) => self.observe_tty(id.clone()),
            InternalEndpointId::Mock(_) => self.observe_mock(id.clone()),
        };

        if reply.is_ok() {
            self.set_observing(&user, id);
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
            .entry(id.clone())
            .or_insert_with(|| Box::new(MockBuilder::new(mock_id).build()));
        // .or_insert_with(|| Box::new(MockHandle::run(mock_id)));
        let control_context = ControlContext {
            user_request: UserRequest::EndpointId(EndpointId::from(id)),
            got_control: None,
        };

        // TODO: Share impl with tty
        let semaphore = endpoint.semaphore();
        let id = endpoint.semaphore_id();
        let endpoints = self.endpoints_controlled_by(&id);

        let maybe_control = match semaphore.clone().inner.try_acquire_owned() {
            Ok(permit) => MaybeEndpointController::available(
                control_context,
                EndpointController {
                    _permit: permit,
                    endpoints,
                    // id,
                },
            ),
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
                                // id,
                            })
                            .is_err()
                        {
                            warn!("Permit acquired but no user to receive it")
                        };
                    } else {
                        warn!("Could not get permit- endpoint closed?")
                    }
                });

                MaybeEndpointController::busy(
                    control_context,
                    EndpointControllerQueue {
                        inner: permit_rx,
                        endpoints: endpoints.keys().cloned().collect(),
                    },
                )
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

    // Check if the semaphore id matching the id is already granted or requested by the user
    fn control_requested_or_given(&self, user: &User, id: &InternalEndpointId) -> bool {
        if let Some(semaphore_id) = self.endpoint_semaphore_id(id) {
            let us = self.user_state(user);

            us.in_queue_of.contains(id) || us.in_control_of.contains(&semaphore_id)
        } else {
            false
        }
    }

    fn control(
        &mut self,
        user: &User,
        id: InternalEndpointId,
    ) -> Result<MaybeEndpointController, Error> {
        if self.control_requested_or_given(user, &id) {
            let error_message =
                format!("User {user} is already queued or already has control over {id}.");

            return Err(Error::SuperfluousRequest(error_message));
        }

        let reply = match &id {
            InternalEndpointId::Tty(_) => self.control_tty(id.clone()),
            InternalEndpointId::Mock(_) => self.control_mock(id.clone()),
        };

        if let Ok(maybe) = &reply {
            if let AvailableOrBusyEndpointController::Busy(controller_queue) = &maybe.inner {
                debug!("Was busy");
                self.set_in_control_queue(user, controller_queue);
            }
        }
        // TODO: Can be problematic if control is called for many endpoints
        // if reply.is_ok() {
        //     self.events.send_event(UserEvent { user: user.clone(), event: Event });

        //     let controllers = self
        //         .controllers
        //         .entry(self.endpoint_semaphore_id(&id).unwrap())
        //         .or_default();

        //     // It's a bug if we insert the same controller twice.
        //     assert!(controllers.insert(user.clone()));
        // }

        reply
    }

    // Get endpoints matching the given label.
    // The endpoints will be unique in terms of which semaphore they
    // require in order to be controlled.
    // This is done because wanting to control a labelled endpoint within
    // a group implies control over the rest of that group,
    // so there is no need to queue for more than one within this group.
    fn labels_to_endpoint_ids(&self, label: &Label) -> HashSet<InternalEndpointId> {
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

        let control_context = ControlContext {
            user_request: UserRequest::Label(label.clone()),
            got_control: None,
        };

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
            let mut error_message = String::from("All matching endpoints: [");
            for id in ids {
                error_message += &format!(" {id}");
            }
            error_message += " ] resulted in errors: [";
            for e in errs {
                error_message += &format!(" <{e}>");
            }
            error_message += " ]";

            return Err(Error::BadUsage(error_message));
        }

        let (available, busy): (Vec<_>, Vec<_>) =
            oks.into_iter()
                .partition_map(|maybe_controller| match maybe_controller.inner {
                    AvailableOrBusyEndpointController::Available(_) => {
                        Either::Left(maybe_controller)
                    }
                    AvailableOrBusyEndpointController::Busy(busy) => Either::Right(busy),
                });
        if let Some(mut controller) = available.into_iter().next() {
            controller.context = control_context;

            Ok(controller)
        } else {
            assert!(!busy.is_empty());

            let queued_endpoints = busy
                .iter()
                .flat_map(|queue| queue.endpoints.clone())
                .collect::<Vec<_>>();

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

            Ok(MaybeEndpointController::busy(
                control_context,
                EndpointControllerQueue {
                    inner: controller_rx,
                    endpoints: queued_endpoints,
                },
            ))
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

    // Mock endpoints which were not a part of a config file
    // are transient and isolated to one user's session.
    fn remove_dangling_mock_endpoints(&mut self) {
        let inactive = {
            let all_controlled = self
                .user_state
                .values()
                .flat_map(|state| &state.in_control_of)
                .flat_map(|e| self.semaphore_id_to_endpoints(e))
                .filter(|e| self.endpoints.get(e).unwrap().labels().is_none())
                .collect::<HashSet<_>>();

            let all_queued = self
                .user_state
                .values()
                .flat_map(|state| &state.in_queue_of)
                .filter(|e| self.endpoints.get(e).unwrap().labels().is_none())
                .cloned()
                .collect::<HashSet<_>>();

            let all_active = all_controlled
                .union(&all_queued)
                .cloned()
                .collect::<HashSet<_>>();

            let all_non_labelled = self
                .endpoints
                .iter()
                .filter(|(_, e)| e.labels().is_none())
                .map(|(id, _)| id)
                .cloned()
                .collect::<HashSet<_>>();

            all_non_labelled
                .difference(&all_active)
                .cloned()
                .collect::<Vec<_>>()
        };

        for inactive in inactive {
            debug!(%inactive, "No more observers/controllers for mock (using or queued), removing");
            assert!(self.endpoints.remove(&inactive).is_some());
        }
    }

    fn semaphore_id_to_endpoints(&self, id: &EndpointSemaphoreId) -> Vec<InternalEndpointId> {
        self.endpoints
            .values()
            .filter(|e| &e.semaphore_id() == id)
            .map(|e| e.internal_endpoint_id())
            .collect()
    }

    fn handle_information(&mut self, information: Inform) {
        match information {
            Inform::UserLeft(user) => {
                let _span = debug_span!("User leaving", %user).entered();

                let mut state = self.user_state.remove(&user).expect("User was alive");

                let observing = state.observing.drain().collect::<Vec<_>>();
                if !observing.is_empty() {
                    self.events.send_event(UserEvent {
                        user: user.clone(),
                        event: Event::NoLongerObserving(observing),
                    });
                }

                let in_queue_for = state.in_queue_of.drain().collect::<Vec<_>>();
                if !in_queue_for.is_empty() {
                    self.events.send_event(UserEvent {
                        user: user.clone(),
                        event: Event::NoLongerInQueueOf(in_queue_for),
                    });
                }

                let controlling = state.in_control_of.drain().collect::<Vec<_>>();
                if !controlling.is_empty() {
                    self.events.send_event(UserEvent {
                        user: user.clone(),
                        event: Event::NoLongerInControlOf(
                            controlling
                                .into_iter()
                                .flat_map(|semaphore_id| {
                                    self.semaphore_id_to_endpoints(&semaphore_id)
                                })
                                .collect(),
                        ),
                    });
                }

                self.events.send_event(UserEvent {
                    user,
                    event: Event::Left,
                });

                self.remove_dangling_mock_endpoints();
            }
            Inform::NowControlling { user, context } => {
                let _span = info_span!("NowControlling", %user, %context).entered();

                let which = context.got_control.expect(
                    "Which endpoints are controlled should be part of the sent information",
                );
                debug!(?which, "These are now controlled");

                if let UserRequest::Label(label) = context.user_request {
                    let _label_span = info_span!("Label", %label).entered();

                    // These are now controlled
                    let user_controls_set: HashSet<InternalEndpointId> =
                        HashSet::from_iter(which.clone());

                    // These are all matching the label
                    let matches_label_set = self.labels_to_endpoint_ids(&label);
                    debug!(?matches_label_set, "The label matches these endpoints");

                    // This difference represents all which this user is in queue for,
                    // minus the ones they now control.
                    // This set should be removed from the currently queued endpoints.
                    let difference = matches_label_set
                        .difference(&user_controls_set)
                        .cloned()
                        .collect::<Vec<_>>();
                    debug!(?difference, "The difference");

                    if !difference.is_empty() {
                        self.events.send_event(UserEvent {
                            user: user.clone(),
                            event: Event::NoLongerInQueueOf(difference.clone()),
                        });

                        let in_queue_of = &self.user_state(&user).in_queue_of;
                        debug!(?in_queue_of, "Current queue");

                        let new_queue = in_queue_of
                            .difference(&HashSet::from_iter(difference))
                            .cloned()
                            .collect();
                        debug!(?new_queue, "New queue");

                        self.user_state_mut(&user).in_queue_of = new_queue;
                    }
                }

                self.set_controls(&user, which);
            }
            Inform::UserArrived(user) => {
                info!(%user, "New user");
                assert!(self
                    .user_state
                    .insert(user.clone(), UserState::default())
                    .is_none());

                self.events.send_event(UserEvent {
                    user,
                    event: Event::Connected,
                });
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
        let user = User::new("fooo");
        cc.inform(Inform::UserArrived(user.clone()));

        let response = cc
            .perform_action(
                user,
                Action::Observe(InternalEndpointId::Mock(MockId::new("user1", "mock"))),
            )
            .await
            .unwrap();

        assert!(matches!(response, ControlCenterResponse::ObserveThis(_)));
    }

    #[tokio::test]
    async fn observe_non_existing_tty_id() {
        let mut cc = cc();
        let user = User::new("foo");
        cc.inform(Inform::UserArrived(user.clone()));

        let response = cc
            .perform_action(
                user,
                Action::Observe(InternalEndpointId::Tty("/dev/tty1234".into())),
            )
            .await;

        assert!(matches!(response, Err(Error::NoSuchEndpoint(_))));
    }

    #[tokio::test]
    async fn can_not_observe_mock_endpoint_several_times() {
        let mut cc = cc();

        let user = User::new("Foo");
        cc.inform(Inform::UserArrived(user.clone()));

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
