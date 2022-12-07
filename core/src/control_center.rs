//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet, VecDeque},
    fmt::{Debug, Display},
};

use futures::{channel::mpsc, SinkExt, StreamExt};
use itertools::{Either, Itertools};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, OwnedSemaphorePermit, TryAcquireError};
use tracing::{debug, debug_span, info, info_span, warn};

use crate::{
    config::{Config, ConfigEndpoint},
    endpoint::{
        Endpoint, EndpointExt, EndpointId, EndpointSemaphore, EndpointSemaphoreId,
        InternalEndpointId, InternalEndpointInfo, LabelledEndpointId, Labels,
    },
    error::Error,
    mock::{MockBuilder, MockId},
    serial::{serial_port::SerialPortBuilder, SerialMessage, SerialMessageBytes},
    user::User,
};

#[derive(Debug)]
pub(crate) struct EndpointController {
    _permit: OwnedSemaphorePermit,
    pub(crate) endpoints: HashMap<InternalEndpointInfo, mpsc::UnboundedSender<SerialMessageBytes>>,
}

#[derive(Debug)]
pub(crate) struct EndpointControllerQueue {
    pub(crate) inner: oneshot::Receiver<EndpointController>,
    pub(crate) endpoints: Vec<InternalEndpointInfo>,
}

impl EndpointControllerQueue {
    fn endpoints_infos(&self) -> Vec<InternalEndpointInfo> {
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
}

/// Did the user request access to
/// a specific endpoint, or a label
#[derive(Debug, Clone)]
pub(crate) enum UserRequest {
    EndpointId(EndpointId),
    Labels(Labels),
}

/// The context of getting access to controlling
/// something.
#[derive(Debug, Clone)]
pub(crate) struct ControlContext {
    /// The originating request.
    user_request: UserRequest,

    /// When the request resolves,
    /// which endpoints were gained control over.
    pub(crate) got_control: Option<Vec<InternalEndpointInfo>>,
}

impl Display for ControlContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.user_request {
            UserRequest::EndpointId(id) => write!(f, "{id}"),
            UserRequest::Labels(labels) => write!(f, "{labels}"),
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

/// An event connected to some user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserEvent {
    /// The user related to this event.
    pub user: User,

    /// The event.
    pub event: Event,

    /// When the event happened.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl UserEvent {
    /// Create a new user event.
    pub fn new(user: &User, event: Event) -> Self {
        Self {
            user: user.clone(),
            event,
            timestamp: chrono::Utc::now(),
        }
    }
}

impl Display for UserEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{user}: {event}", user = self.user, event = self.event)
    }
}

/// Events that can happen to a user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Event {
    /// A user has connected.
    Connected,

    /// A user has disconnected.
    Disconnected,

    /// A user sent (i.e. put on wire) this message.
    MessageSent((InternalEndpointInfo, SerialMessage)),
    /// A user received (i.e. got from wire) this message.
    MessageReceived((InternalEndpointInfo, SerialMessage)),

    /// A user is now observing some endpoints.
    Observing(Vec<InternalEndpointInfo>),

    /// A user is no longer observing some endpoints.
    NoLongerObserving(Vec<InternalEndpointInfo>),

    /// A user is now in queue for some endpoints.
    InQueueFor(Vec<InternalEndpointInfo>),

    /// A user is now in control of some endpoints.
    InControlOf(Vec<InternalEndpointInfo>),

    /// A user is no longer in queue for some endpoints.
    NoLongerInQueueOf(Vec<InternalEndpointInfo>),

    /// A user is no longer in control of some endpoints.
    NoLongerInControlOf(Vec<InternalEndpointInfo>),
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut write_endpoints = |prefix: &str, endpoints: &[InternalEndpointInfo]| {
            let endpoints = endpoints
                .iter()
                .map(|endpoint| format!("{}", endpoint))
                .join(", ");
            write!(f, "{prefix} {}", endpoints)
        };

        match self {
            Event::Connected => write!(f, "connected"),
            Event::Disconnected => write!(f, "disconnected"),
            Event::Observing(endpoints) => write_endpoints("observing", endpoints),
            Event::NoLongerObserving(endpoints) => {
                write_endpoints("no longer observing", endpoints)
            }
            Event::InQueueFor(endpoints) => write_endpoints("in queue for", endpoints),
            Event::InControlOf(endpoints) => write_endpoints("in control of", endpoints),
            Event::NoLongerInQueueOf(endpoints) => {
                write_endpoints("no longer in queue for", endpoints)
            }
            Event::NoLongerInControlOf(endpoints) => {
                write_endpoints("no longer in control of", endpoints)
            }
            Event::MessageSent((info, msg)) => write!(f, "sent: {msg} to {info}"),
            Event::MessageReceived((info, msg)) => write!(f, "received: {msg} to {info}"),
        }
    }
}

#[derive(Debug, Default)]
struct UserState {
    observing_endpoints: HashSet<InternalEndpointInfo>,
    observing_user_events: bool,
    in_queue_of: HashSet<InternalEndpointInfo>,
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
        let (tx, rx) = broadcast::channel(100000);
        Self {
            tx,
            rx,
            log: VecDeque::new(),
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<UserEvent> {
        self.tx.subscribe()
    }

    fn send_event(&mut self, event: UserEvent) {
        info!(%event, "Sending and storing event");
        self.log.push_front(event.clone());

        // Keep a log of at most this number recent events.
        // Truncate removes from the back, which means older events are split off first.
        self.log.truncate(1000);

        self.tx.send(event).expect("Broadcast should work");
    }
}

#[derive(Default)]
pub(crate) struct Endpoints(HashMap<InternalEndpointInfo, Box<dyn Endpoint + Send + Sync>>);

impl Endpoints {
    fn insert(&mut self, id: InternalEndpointId, endpoint: impl Endpoint + Send + Sync + 'static) {
        let labels = endpoint.labels();
        debug!(?labels, %id, "Adding endpoint");

        self.0.insert(
            InternalEndpointInfo::new(id, endpoint.labels()),
            Box::new(endpoint),
        );
    }

    fn create_mock(&self, mock_id: &MockId) -> impl Endpoint {
        debug!(%mock_id, "Creating mock");
        MockBuilder::new(mock_id.clone()).build()
    }

    fn get_or_create_mock(&mut self, mock_id: &MockId) -> &dyn Endpoint {
        let id = InternalEndpointId::Mock(mock_id.clone());

        if self.0.get(&id).is_none() {
            self.insert(id.clone(), self.create_mock(mock_id));
        }

        // Borrow of a box
        let e = self.0.get(&id).unwrap();

        // Get the box, get the contents, re-borrow to re-interpret (I think)
        &**e
    }

    fn get<B>(&self, internal_endpoint: B) -> Result<&dyn Endpoint, Error>
    where
        B: Borrow<InternalEndpointId> + Display,
    {
        match self.0.get(internal_endpoint.borrow()) {
            Some(e) => Ok(&**e),
            None => Err(Error::NoSuchEndpoint(internal_endpoint.to_string())),
        }
    }

    /// Get the [`InternalEndpointInfo`] from an [`InternalEndpointId`], if the
    /// endpoint exists.
    fn id_to_info(&self, id: InternalEndpointId) -> Result<InternalEndpointInfo, Error> {
        Ok(InternalEndpointInfo::new(
            id.clone(),
            self.get(id)?.labels(),
        ))
    }

    fn remove(&mut self, id: &InternalEndpointInfo) {
        assert!(self.0.remove(id).is_some());
    }

    fn without_labels(&self) -> HashSet<InternalEndpointInfo> {
        self.0
            .keys()
            .filter(|info| info.labels.is_none())
            .cloned()
            .collect::<HashSet<_>>()
    }

    fn endpoints_with_semaphore_id(
        &self,
        semaphore_id: &EndpointSemaphoreId,
    ) -> Vec<InternalEndpointInfo> {
        self.0
            .iter()
            .filter_map(|(id, e)| {
                if &e.semaphore_id() == semaphore_id {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn endpoint_message_sender(
        &self,
        info: &InternalEndpointInfo,
    ) -> mpsc::UnboundedSender<SerialMessageBytes> {
        self.get(info.borrow())
            .expect("This should only be used on known existing endpoint")
            .message_sender()
    }

    fn endpoint_semaphore_id(&self, id: &InternalEndpointInfo) -> Option<EndpointSemaphoreId> {
        self.0.get(id).map(|endpoint| endpoint.semaphore_id())
    }

    fn semaphore_id_to_endpoints(&self, id: &EndpointSemaphoreId) -> Vec<InternalEndpointInfo> {
        self.0
            .iter()
            .filter(|(_, e)| &e.semaphore_id() == id)
            .map(|(info, _)| info)
            .cloned()
            .collect()
    }

    // Get endpoints matching the given label.
    // The endpoints will be unique in terms of which semaphore they
    // require in order to be controlled.
    // This is done because wanting to control a labelled endpoint within
    // a group implies control over the rest of that group,
    // so there is no need to queue for more than one within this group.
    fn labels_to_endpoint_ids(&self, labels: &Labels) -> HashSet<InternalEndpointInfo> {
        self.0
            .iter()
            .filter_map(|(info, endpoint)| endpoint.labels().map(|labels| (info, labels)))
            .filter(|(_, endpoint_labels)| endpoint_labels.is_superset(labels))
            .map(|(info, _)| info)
            .unique_by(|info| self.endpoint_semaphore_id(info).expect("Endpoint exists"))
            .cloned()
            .collect()
    }
}

pub(crate) struct ControlCenter {
    /// Messages for the control center to handle.
    messages: mpsc::UnboundedReceiver<ControlCenterMessage>,

    /// [`Event`]s broadcasted.
    events: Events,

    /// The [`Endpoint`]s handled here.
    endpoints: Endpoints,

    /// The state of each live user.
    user_state: HashMap<User, UserState>,
}

/// Actions available to ask of the control center.
#[derive(Debug)]
pub(crate) enum Action {
    Observe(InternalEndpointId),
    Control(InternalEndpointId),
    ControlAny(Labels),
    SubscribeToUserEvents,
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Observe(id) => write!(f, "observe: {id}"),
            Action::Control(id) => write!(f, "control: {id}"),
            Action::ControlAny(labels) => {
                write!(f, "control any: {labels}")
            }
            Action::SubscribeToUserEvents => write!(f, "subscribe to user events"),
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

    MessageReceived((User, InternalEndpointInfo, SerialMessage)),
    MessageSent((User, InternalEndpointId, SerialMessage)),

    NowControlling {
        user: User,
        context: ControlContext,
    },
}

impl Display for Inform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inform::UserArrived(user) => write!(f, "user arrived: {user}"),
            Inform::UserLeft(user) => write!(f, "user left: {user}"),
            Inform::NowControlling { user, context } => {
                write!(f, "{user} now controlling, ctx: {context}")
            }
            Inform::MessageReceived((user, info, msg)) => {
                write!(f, "user {user} got message {msg} via {info}")
            }
            Inform::MessageSent((user, info, msg)) => {
                write!(f, "user {user} sent message {msg} via {info}")
            }
        }
    }
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
    EndpointObserver(
        (
            InternalEndpointInfo,
            broadcast::Receiver<SerialMessageBytes>,
        ),
    ),
    UserEventObserver(broadcast::Receiver<UserEvent>),
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
        let _span = info_span!("ControlCenter init").entered();

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

        let mut endpoints = Endpoints::default();

        for ConfigEndpoint {
            id: endpoint_id,
            labels,
        } in config.endpoints
        {
            match endpoint_id {
                EndpointId::Tty(tty) => {
                    let mut builder = SerialPortBuilder::new(&tty);

                    for label in labels.into_iter() {
                        builder = builder.add_label(label);
                    }

                    let id = InternalEndpointId::Tty(tty);
                    endpoints.insert(id, builder.build());
                }
                EndpointId::Mock(mock) => {
                    let mock_id = MockId::new("MockFromConfig", &mock);
                    let id = InternalEndpointId::Mock(mock_id.clone());

                    let mut builder = MockBuilder::new(mock_id);
                    for label in labels.into_iter() {
                        builder = builder.add_label(label);
                    }

                    endpoints.insert(id, builder.build());
                }
            }
        }

        for (index, group) in config.groups.iter().enumerate() {
            let shared_semaphore = EndpointSemaphore::default();

            let group_label = &group.labels;

            if group.is_mock_group() {
                let group_name = format!("MockGroup{index}");

                for config_endpoint in &group.endpoints {
                    let endpoint_name = config_endpoint.id.as_mock().unwrap();

                    let mock_id = MockId::new(&group_name, endpoint_name);
                    let id = InternalEndpointId::Mock(mock_id.clone());

                    let mut builder =
                        MockBuilder::new(mock_id).set_semaphore(shared_semaphore.clone());

                    for label in group_label.iter() {
                        builder = builder.add_label(label.clone());
                    }

                    for label in config_endpoint.labels.iter() {
                        builder = builder.add_label(label.clone());
                    }

                    endpoints.insert(id, builder.build());
                }
            } else {
                for config_endpoint in &group.endpoints {
                    let tty_path = config_endpoint.id.as_tty().unwrap();
                    let mut builder =
                        SerialPortBuilder::new(tty_path).set_semaphore(shared_semaphore.clone());

                    for label in group_label.iter() {
                        builder = builder.add_label(label.clone());
                    }

                    for label in config_endpoint.labels.iter() {
                        builder = builder.add_label(label.clone());
                    }

                    let id = InternalEndpointId::Tty(tty_path.into());
                    endpoints.insert(id, builder.build());
                }
            }
        }

        // TODO: We have to remove any in a group
        for port in &available {
            let id = InternalEndpointId::Tty(port.clone());
            info!(
                "Auto (not specified in config file) setting up endpoint for {}",
                id
            );
            let endpoint = SerialPortBuilder::new(port).build();

            endpoints.insert(id, endpoint);
        }

        Self {
            messages: requests,
            endpoints,
            events: Events::new(),
            user_state: HashMap::new(),
        }
    }

    fn is_observing_endpoint(&self, user: &User, id: &InternalEndpointInfo) -> bool {
        self.user_state
            .get(user)
            .expect("We should know about live users")
            .observing_endpoints
            .contains(id)
    }

    fn is_observing_user_events(&self, user: &User) -> bool {
        self.user_state
            .get(user)
            .expect("We should know about live users")
            .observing_user_events
    }

    fn set_observing_user_events(&mut self, user: &User) {
        self.user_state
            .get_mut(user)
            .expect("We should know about live users")
            .observing_user_events = true;
    }

    fn set_observing_endpoint(&mut self, user: &User, id: InternalEndpointInfo) {
        // Assert: Just making sure we don't double insert,
        // which would be a bug on our part.
        assert!(self
            .user_state_mut(user)
            .observing_endpoints
            .insert(id.clone()));

        self.events
            .send_event(UserEvent::new(user, Event::Observing(vec![id])));
    }

    fn user_state_mut(&mut self, user: &User) -> &mut UserState {
        self.user_state.get_mut(user).expect("User should be alive")
    }

    fn user_state(&self, user: &User) -> &UserState {
        self.user_state.get(user).expect("User should be alive")
    }

    fn set_controls(&mut self, user: &User, endpoints_infos: Vec<InternalEndpointInfo>) {
        info_span!("Now controls", %user);

        self.events.send_event(UserEvent::new(
            user,
            Event::InControlOf(endpoints_infos.clone()),
        ));

        let mut semaphore_ids = endpoints_infos
            .into_iter()
            .map(|id| self.endpoints.endpoint_semaphore_id(&id).expect("Exists"))
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
        let endpoint_ids = controller_queue.endpoints_infos();

        self.events.send_event(UserEvent::new(
            user,
            Event::InQueueFor(endpoint_ids.clone()),
        ));

        for id in endpoint_ids {
            assert!(self.user_state_mut(user).in_queue_of.insert(id));
        }
    }

    fn observe(
        &mut self,
        user: User,
        id: InternalEndpointId,
    ) -> Result<ControlCenterResponse, Error> {
        let to_observe = if let InternalEndpointId::Mock(mock_id) = &id {
            self.endpoints.get_or_create_mock(mock_id)
        } else {
            self.endpoints.get(id.borrow())?
        }
        .inbox();

        let info = self.endpoints.id_to_info(id.clone())?;

        if self.is_observing_endpoint(&user, &info) {
            return Err(Error::SuperfluousRequest(format!(
                "`{user}` is already observing endpoint `{}`",
                LabelledEndpointId::from(info)
            )));
        }

        self.set_observing_endpoint(&user, info.clone());

        Ok(ControlCenterResponse::EndpointObserver((info, to_observe)))
    }

    fn control_impl(
        &mut self,
        info: InternalEndpointInfo,
    ) -> Result<MaybeEndpointController, Error> {
        let endpoint = self
            .endpoints
            .get(info.borrow())
            .expect("Should check existence before this");

        let (semaphore, semaphore_id) = (endpoint.semaphore(), endpoint.semaphore_id());

        let control_context = ControlContext {
            user_request: UserRequest::EndpointId(EndpointId::from(info.id)),
            got_control: None,
        };

        let endpoints_with_semaphore = self
            .endpoints
            .endpoints_with_semaphore_id(&semaphore_id)
            .into_iter()
            .map(|info| (self.endpoints.endpoint_message_sender(&info), info))
            .map(|(tx, info)| (info, tx))
            .collect();

        let maybe_control = match semaphore.clone().inner.try_acquire_owned() {
            Ok(permit) => MaybeEndpointController::available(
                control_context,
                EndpointController {
                    _permit: permit,
                    endpoints: endpoints_with_semaphore,
                },
            ),
            Err(TryAcquireError::NoPermits) => {
                let (permit_tx, permit_rx) = oneshot::channel();
                let permit_fut = semaphore.inner.acquire_owned();
                let task_endpoints = endpoints_with_semaphore.clone();

                tokio::spawn(async move {
                    if let Ok(permit) = permit_fut.await {
                        if permit_tx
                            .send(EndpointController {
                                _permit: permit,
                                endpoints: task_endpoints,
                            })
                            .is_err()
                        {
                            debug!("Permit acquired but no user to receive it")
                        };
                    } else {
                        warn!("Could not get permit- endpoint closed?")
                    }
                });

                MaybeEndpointController::busy(
                    control_context,
                    EndpointControllerQueue {
                        inner: permit_rx,
                        endpoints: endpoints_with_semaphore.keys().cloned().collect(),
                    },
                )
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        };

        Ok(maybe_control)
    }

    // Check if the semaphore id matching the id is already granted or requested by the user
    fn control_requested_or_given(&self, user: &User, info: &InternalEndpointInfo) -> bool {
        if let Some(semaphore_id) = self.endpoints.endpoint_semaphore_id(info) {
            let us = self.user_state(user);

            us.in_queue_of.contains(info) || us.in_control_of.contains(&semaphore_id)
        } else {
            false
        }
    }

    fn control(
        &mut self,
        user: &User,
        id: InternalEndpointId,
    ) -> Result<MaybeEndpointController, Error> {
        if let InternalEndpointId::Mock(mock_id) = &id {
            self.endpoints.get_or_create_mock(mock_id);
        };

        let info = self.endpoints.id_to_info(id)?;

        if self.control_requested_or_given(user, &info) {
            let error_message =
                format!("User {user} is already queued or already has control over {info}.");

            return Err(Error::SuperfluousRequest(error_message));
        }
        let reply = self.control_impl(info);

        if let Ok(maybe) = &reply {
            if let AvailableOrBusyEndpointController::Busy(controller_queue) = &maybe.inner {
                debug!("Was busy");
                self.set_in_control_queue(user, controller_queue);
            }
        }

        reply
    }

    fn control_any(
        &mut self,
        user: User,
        labels: Labels,
    ) -> Result<MaybeEndpointController, Error> {
        // Here's the "algorithm" for controlling any matching endpoint.
        //
        //  1.  Get list of endpoints matching the labels
        //  2.  If empty, quit
        //  3.  Attempt controlling all matching ones
        //  4.  If only errors, quit
        //  5.  If at least one is available without a queue, use the first one, quit
        //  6.  Else: Make a queue which yields the first one.

        if labels.is_empty() {
            return Err(Error::BadUsage(
                "At least one label must be provided".to_string(),
            ));
        }

        let control_context = ControlContext {
            user_request: UserRequest::Labels(labels.clone()),
            got_control: None,
        };

        let infos = self.endpoints.labels_to_endpoint_ids(&labels);
        if infos.is_empty() {
            return Err(Error::NoMatchingEndpoints(labels));
        }

        let (oks, errs): (Vec<_>, Vec<_>) = infos
            .clone()
            .into_iter()
            .map(|info| self.control(&user, info.id))
            .partition_result();

        if oks.is_empty() {
            let mut error_message = String::from("All matching endpoints: [");
            for id in infos {
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

    fn subscribe_to_user_events(&mut self, user: &User) -> Result<ControlCenterResponse, Error> {
        if self.is_observing_user_events(user) {
            return Err(Error::BadUsage(
                "User is already subscribed to user events".to_string(),
            ));
        };
        self.set_observing_user_events(user);

        Ok(ControlCenterResponse::UserEventObserver(
            self.events.subscribe(),
        ))
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
            Action::ControlAny(labels) => self
                .control_any(user, labels)
                .map(ControlCenterResponse::ControlThis),
            Action::SubscribeToUserEvents => self.subscribe_to_user_events(&user),
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
                .flat_map(|e| self.endpoints.semaphore_id_to_endpoints(e))
                .filter(|info| info.labels.is_none())
                .collect::<HashSet<_>>();

            let all_queued = self
                .user_state
                .values()
                .flat_map(|state| &state.in_queue_of)
                .filter(|info| info.labels.is_none())
                .cloned()
                .collect::<HashSet<_>>();

            let all_active = all_controlled
                .union(&all_queued)
                .cloned()
                .collect::<HashSet<_>>();

            let all_non_labelled = self.endpoints.without_labels();

            all_non_labelled
                .difference(&all_active)
                .cloned()
                .collect::<Vec<_>>()
        };

        for inactive in inactive {
            debug!(%inactive, "No more observers/controllers for mock (using or queued), removing");
            self.endpoints.remove(&inactive);
        }
    }

    fn handle_information(&mut self, information: Inform) {
        debug!(%information, "Got information");

        match information {
            Inform::UserLeft(user) => {
                let _span = debug_span!("User leaving", %user).entered();

                let mut state = self.user_state.remove(&user).expect("User was alive");

                let observing = state.observing_endpoints.drain().collect::<Vec<_>>();
                if !observing.is_empty() {
                    self.events
                        .send_event(UserEvent::new(&user, Event::NoLongerObserving(observing)));
                }

                let in_queue_for = state.in_queue_of.drain().collect::<Vec<_>>();
                if !in_queue_for.is_empty() {
                    self.events.send_event(UserEvent::new(
                        &user,
                        Event::NoLongerInQueueOf(in_queue_for),
                    ));
                }

                let controlling = state.in_control_of.drain().collect::<Vec<_>>();
                if !controlling.is_empty() {
                    self.events.send_event(UserEvent::new(
                        &user,
                        Event::NoLongerInControlOf(
                            controlling
                                .into_iter()
                                .flat_map(|semaphore_id| {
                                    self.endpoints.semaphore_id_to_endpoints(&semaphore_id)
                                })
                                .collect(),
                        ),
                    ));
                }

                self.events
                    .send_event(UserEvent::new(&user, Event::Disconnected));

                self.remove_dangling_mock_endpoints();
            }
            Inform::NowControlling { user, context } => {
                let _span = info_span!("NowControlling", %user, %context).entered();

                let which = context.got_control.expect(
                    "Which endpoints are controlled should be part of the sent information",
                );
                debug!(?which, "These are now controlled");

                if let UserRequest::Labels(labels) = context.user_request {
                    let _label_span = info_span!("Labels", ?labels).entered();

                    // These are now controlled
                    let user_controls_set: HashSet<InternalEndpointInfo> =
                        HashSet::from_iter(which.clone());

                    // These are all matching the label
                    let matches_label_set = self.endpoints.labels_to_endpoint_ids(&labels);
                    debug!(?matches_label_set, "The label matches these endpoints");

                    // This difference represents all which this user is in queue for,
                    // minus the ones they now control.
                    // This set should be removed from the currently queued endpoints.
                    let difference = matches_label_set
                        .difference(&user_controls_set)
                        .cloned()
                        .collect::<Vec<_>>();
                    debug!(?difference, "The difference");

                    // BUG: This does not fire at the correct time.
                    // Queue event only sent when everything is over
                    if !difference.is_empty() {
                        self.events.send_event(UserEvent::new(
                            &user,
                            Event::NoLongerInQueueOf(difference.clone()),
                        ));

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

                self.events
                    .send_event(UserEvent::new(&user, Event::Connected));
            }
            // Inform::MessageReceived(_) => {}
            // Inform::MessageSent(_) => {}
            Inform::MessageReceived((user, info, msg)) => self
                .events
                .send_event(UserEvent::new(&user, Event::MessageReceived((info, msg)))),
            Inform::MessageSent((user, id, msg)) => self.events.send_event(UserEvent::new(
                &user,
                Event::MessageSent((
                    self.endpoints
                        .id_to_info(id)
                        .expect("Message should be sent to a known endpoint"),
                    msg,
                )),
            )),
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

        assert!(matches!(
            response,
            ControlCenterResponse::EndpointObserver(_)
        ));
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
                    Ok(ControlCenterResponse::EndpointObserver(_))
                ));
            } else {
                assert!(matches!(response, Err(Error::SuperfluousRequest(_))));
            }
        }
    }
}
