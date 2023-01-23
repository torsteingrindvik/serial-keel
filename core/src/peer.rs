use async_recursion::async_recursion;
use futures::SinkExt;
use itertools::Itertools;
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
};
use tracing::{debug, info, info_span, warn, Instrument};

use crate::{
    actions::{self, ResponseResult},
    control_center::{
        self, ControlCenterHandle, EndpointController, EndpointControllerQueue, Inform,
    },
    endpoint::{
        self, EndpointId, InternalEndpointId, InternalEndpointInfo, LabelledEndpointId, Labels,
    },
    error, events,
    mock::MockId,
    serial::{SerialMessage, SerialMessageBytes},
    user::User,
};

pub(crate) struct Peer {
    // Which user does this peer represent
    user: User,

    // Where to put responses to this peer's requests
    sender: mpsc::UnboundedSender<ResponseResult>,

    // For sending requests to us
    peer_requests_sender: mpsc::UnboundedSender<PeerRequest>,

    // For reading requests to us
    peer_requests_receiver: mpsc::UnboundedReceiver<PeerRequest>,

    // The endpoints this peer controls
    controllers: Vec<EndpointController>,

    // The handle to the control center,
    // which holds global state.
    cc_handle: ControlCenterHandle,
}

// TODO: Close this gracefully?
async fn endpoint_handler(
    user: User,
    cc_handle: ControlCenterHandle,
    info: InternalEndpointInfo,
    mut endpoint_events: broadcast::Receiver<endpoint::EndpointEvent>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
    info!("Starting handler for {user}+{info}");

    while let Ok(event) = endpoint_events.recv().await {
        let Some(message) = event.as_from_wire() else { continue };

        if user_sender
            .send(Ok(actions::Response::message(
                info.clone().into(),
                message.clone(),
            )))
            .is_err()
        {
            debug!("Send error");
            break;
        }

        cc_handle.inform(Inform::UserReceivedMessage((
            user.clone(),
            info.clone(),
            SerialMessage::new_lossy(message),
        )))
    }

    debug!("Endpoint {info} closed")
}

// TODO: Close this gracefully?
async fn event_handler(
    mut events: broadcast::Receiver<events::TimestampedEvent>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
    info!("Starting user event handler");

    while let Ok(event) = events.recv().await {
        if user_sender
            .send(Ok(actions::Response::Async(actions::Async::Event(event))))
            .is_err()
        {
            debug!("Send error");
            break;
        }
    }

    debug!("User event handler closed")
}

#[derive(Debug)]
pub(crate) enum PeerAction {
    /// An outbox we're waiting for is now ready.
    ControllerReady {
        context: control_center::ControlContext,
        controller: EndpointController,
    },

    /// Shut down the peer, cleaning up as necessary.
    Shutdown,
}

#[derive(Debug)]
pub(crate) enum PeerRequest {
    UserAction(actions::Action),
    InternalAction(PeerAction),
}

pub(crate) struct PeerHandle {
    pub(crate) requests: mpsc::UnboundedSender<PeerRequest>,
    pub(crate) join_handle: JoinHandle<()>,
}

impl PeerHandle {
    pub(crate) fn new(
        user: User,
        sender: mpsc::UnboundedSender<ResponseResult>,
        cc_handle: ControlCenterHandle,
    ) -> Self {
        let (peer_requests_sender, peer_requests_receiver) = mpsc::unbounded_channel();

        let mut peer = Peer::new(
            user,
            sender,
            peer_requests_sender.clone(),
            peer_requests_receiver,
            cc_handle,
        );

        let peer_handle =
            tokio::spawn(async move { peer.run().await }.instrument(info_span!("Peer")));

        Self {
            requests: peer_requests_sender,
            join_handle: peer_handle,
        }
    }

    pub(crate) fn send(&self, request: actions::Action) {
        self.requests
            .send(PeerRequest::UserAction(request))
            .expect("Task should be alive");
    }

    pub(crate) async fn shutdown(self) {
        debug!("Shutting down");
        self.requests
            .send(PeerRequest::InternalAction(PeerAction::Shutdown))
            .expect("Task should be alive");

        self.join_handle.await.expect("Peer should not panic");
        debug!("Shutdown complete");
    }
}

impl Peer {
    fn new(
        user: User,
        sender: mpsc::UnboundedSender<ResponseResult>,
        peer_requests_sender: mpsc::UnboundedSender<PeerRequest>,
        peer_requests_receiver: mpsc::UnboundedReceiver<PeerRequest>,
        cc_handle: ControlCenterHandle,
    ) -> Self {
        Self {
            user,
            sender,
            controllers: vec![],
            cc_handle,
            peer_requests_receiver,
            peer_requests_sender,
        }
    }

    async fn run(&mut self) {
        self.cc_handle
            .inform(control_center::Inform::UserArrived(self.user.clone()));

        while let Some(peer_request) = self.peer_requests_receiver.recv().await {
            match peer_request {
                PeerRequest::UserAction(action) => {
                    let span = info_span!("Action", %action);
                    let response = self.do_user_action(action).instrument(span).await;
                    self.sender
                        .send(response)
                        .expect("If we're alive it means the websocket connection should be up");
                }
                PeerRequest::InternalAction(PeerAction::Shutdown) => {
                    debug!("Shutting down peer");
                    self.cc_handle
                        .inform(control_center::Inform::UserLeft(self.user.clone()));

                    break;
                }
                PeerRequest::InternalAction(PeerAction::ControllerReady {
                    controller,
                    context,
                }) => {
                    let granted_ids = self.add_endpoint_controller(controller, context);

                    self.sender
                        .send(Ok(actions::Response::control_granted(granted_ids)))
                        .expect("If we're alive it means the websocket connection should be up")
                }
            }
        }
    }

    async fn observe(&mut self, id: InternalEndpointId) -> ResponseResult {
        match self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Observe(id))
            .await
        {
            Ok(control_center::ControlCenterResponse::EndpointObserver((info, endpoint))) => {
                let span = info_span!("Endpoint Handler", %info);

                tokio::spawn(
                    endpoint_handler(
                        self.user.clone(),
                        self.cc_handle.clone(),
                        info.clone(),
                        endpoint,
                        self.sender.clone(),
                    )
                    .instrument(span),
                );

                Ok(actions::Response::observing(vec![
                    LabelledEndpointId::from(info),
                ]))
            }
            Ok(_) => {
                unreachable!()
            }
            Err(e) => Err(e),
        }
    }

    fn mock_id(&self, mock: &str) -> MockId {
        MockId::new(&self.user.name, mock)
    }

    fn add_endpoint_controller(
        &mut self,
        controller: EndpointController,
        mut context: control_center::ControlContext,
    ) -> Vec<LabelledEndpointId> {
        let infos = controller.endpoints.keys().cloned().collect_vec();
        context.got_control = Some(infos.clone());

        self.cc_handle
            .inform(control_center::Inform::NowControlling {
                user: self.user.clone(),
                context,
            });

        self.controllers.push(controller);
        // Log them in their internal representation
        info!(?infos, "Control granted");

        // Now convert to the user/external representation
        infos.into_iter().map(Into::into).collect()
    }

    fn spawn_endpoint_controller_queue_waiter(
        &self,
        queue: oneshot::Receiver<EndpointController>,
        context: control_center::ControlContext,
    ) {
        let peer_reply_sender = self.peer_requests_sender.clone();

        tokio::spawn(
            async move {
                match queue.await {
                    Ok(controller) => {
                        debug!("Controller received after queue");
                        if peer_reply_sender
                            .send(PeerRequest::InternalAction(PeerAction::ControllerReady {
                                controller,
                                context,
                            }))
                            .is_err()
                        {
                            warn!("We received outbox but user left")
                        }
                    }
                    Err(_) => {
                        warn!("Queueing for outbox failed, sender dropped?")
                    }
                }
            }
            .in_current_span(), // Same as peer, which shows user
        );
    }

    async fn handle_control_response(
        &mut self,
        response: Result<control_center::ControlCenterResponse, error::Error>,
    ) -> ResponseResult {
        let response = response?;
        let control_center::MaybeEndpointController { context, inner } = response
            .try_into_control_this()
            .expect("Should get ControlThis response variant");

        match inner {
            control_center::AvailableOrBusyEndpointController::Available(controller) => {
                let granted_ids = self.add_endpoint_controller(controller, context);
                Ok(actions::Response::control_granted(granted_ids))
            }
            control_center::AvailableOrBusyEndpointController::Busy(EndpointControllerQueue {
                inner: queue,
                endpoints,
            }) => {
                self.spawn_endpoint_controller_queue_waiter(queue, context);

                let endpoints = endpoints.into_iter().map(Into::into).collect();
                Ok(actions::Response::control_queue(endpoints))
            }
        }
    }

    async fn control(&mut self, id: InternalEndpointId) -> ResponseResult {
        let response = self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Control(id))
            .await;

        self.handle_control_response(response).await
    }

    async fn control_any(&mut self, labels: Labels) -> ResponseResult {
        let response = self
            .cc_handle
            .perform_action(
                self.user.clone(),
                control_center::Action::ControlAny(labels),
            )
            .await;

        self.handle_control_response(response).await
    }

    fn id_to_internal(&self, endpoint: EndpointId) -> InternalEndpointId {
        match endpoint {
            EndpointId::Tty(tty) => InternalEndpointId::Tty(tty),
            EndpointId::Mock(mock) => InternalEndpointId::Mock(self.mock_id(&mock)),
        }
    }

    async fn write(&mut self, endpoint: EndpointId, message: SerialMessageBytes) -> ResponseResult {
        let user_id = endpoint.clone();
        let id = self.id_to_internal(endpoint);

        let sender = match self
            .controllers
            .iter_mut()
            .flat_map(|controller| controller.endpoints.iter_mut())
            .find(|(info, _)| info.id == id)
            .map(|(_, sender)| sender)
        {
            Some(sender) => Ok(sender),
            None => Err(error::Error::NoPermit(format!("write {user_id}"))),
        }?;

        sender
            .send(message.clone())
            .await
            .expect("Endpoint should be alive");

        self.cc_handle
            .inform(control_center::Inform::UserSentMessage((
                self.user.clone(),
                id,
                String::from_utf8_lossy(&message).into(),
            )));

        Ok(actions::Response::write_ok())
    }

    async fn observe_events(&mut self) -> ResponseResult {
        match self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::SubscribeToEvents)
            .await
        {
            Ok(control_center::ControlCenterResponse::EventObserver(receiver)) => {
                let span = info_span!("Event Handler");

                tokio::spawn(event_handler(receiver, self.sender.clone()).instrument(span));

                Ok(actions::Response::observe_events_ok())
            }
            Ok(_) => {
                unreachable!()
            }
            Err(e) => Err(e),
        }
    }

    #[async_recursion]
    async fn do_user_action(&mut self, action: actions::Action) -> ResponseResult {
        info!("client requested action: {action}");

        match action {
            actions::Action::Observe(id) => self.observe(self.id_to_internal(id)).await,
            actions::Action::Control(id) => self.control(self.id_to_internal(id)).await,
            actions::Action::ControlAny(labels) => self.control_any(labels).await,
            actions::Action::Write((endpoint, message)) => {
                self.write(endpoint, message.into_bytes()).await
            }
            actions::Action::WriteBytes((endpoint, bytes)) => self.write(endpoint, bytes).await,
            actions::Action::ObserveEvents => self.observe_events().await,
        }
    }
}
