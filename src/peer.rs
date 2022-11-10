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
    control_center::{self, ControlCenterHandle, EndpointController, EndpointControllerQueue},
    endpoint::{EndpointId, InternalEndpointId, Label},
    error,
    mock::MockId,
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

    // Tracks which mocks this peer has created (and thus is observing),
    // useful for cleanup when they leave
    // mocks_observing: HashSet<MockId>,

    // Which outboxes the peer may send messages to.
    // These outboxes contain permits, which grants
    // exclusive access.
    // outboxes: HashMap<Endpointid, Outbox>,
    controllers: Vec<EndpointController>,

    // The handle to the control center,
    // which holds global state.
    cc_handle: ControlCenterHandle,
}

// TODO: Close this gracefully?
async fn endpoint_handler(
    id: InternalEndpointId,
    mut endpoint_messages: broadcast::Receiver<String>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
    info!("Starting handler for {id}");

    while let Ok(message) = endpoint_messages.recv().await {
        if user_sender
            .send(Ok(actions::Response::Message {
                endpoint: id.clone().into(),
                message,
            }))
            .is_err()
        {
            debug!("Send error");
            break;
        }
    }

    debug!("Endpoint `{id:?}` closed")
}

#[derive(Debug)]
pub(crate) enum PeerAction {
    /// An outbox we're waiting for is now ready.
    ControllerReady {
        controller: EndpointController,
        // endpoint_id: Endpointid,
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
            // mocks_observing: HashSet::new(),
            controllers: vec![],
            cc_handle,
            peer_requests_receiver,
            peer_requests_sender,
        }
    }

    async fn run(&mut self) {
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
                    info!("Shutting down peer");
                    self.cc_handle
                        .inform(control_center::Inform::UserLeft(self.user.clone()))
                        .await;

                    break;
                }
                PeerRequest::InternalAction(PeerAction::ControllerReady { controller }) => {
                    let granted_ids = self.add_endpoint_controller(controller);

                    self.sender
                        .send(Ok(actions::Response::ControlGranted(granted_ids)))
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
            Ok(control_center::ControlCenterResponse::ObserveThis((id, endpoint))) => {
                let span = info_span!("Endpoint Handler", %id);

                tokio::spawn(endpoint_handler(id, endpoint, self.sender.clone()).instrument(span));

                Ok(actions::Response::Ok)
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

    fn add_endpoint_controller(&mut self, controller: EndpointController) -> Vec<EndpointId> {
        // assert!(self.outboxes.insert(id.clone().into(), outbox).is_none());
        let ids = controller.endpoints.keys().cloned().collect_vec();

        self.controllers.push(controller);
        // Log them in their internal representation
        info!(?ids, "Control granted");

        // Now convert to the user/external representation
        ids.into_iter().map(EndpointId::from).collect()
    }

    fn spawn_endpoint_controller_queue_waiter(&self, queue: oneshot::Receiver<EndpointController>) {
        let peer_reply_sender = self.peer_requests_sender.clone();

        tokio::spawn(
            async move {
                match queue.await {
                    Ok(controller) => {
                        debug!("Controller received after queue");
                        if peer_reply_sender
                            .send(PeerRequest::InternalAction(PeerAction::ControllerReady {
                                controller,
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
        match response {
            Ok(control_center::ControlCenterResponse::ControlThis(maybe_controller)) => {
                match maybe_controller {
                    control_center::MaybeEndpointController::Available(controller) => {
                        let granted_ids = self.add_endpoint_controller(controller);
                        Ok(actions::Response::ControlGranted(granted_ids))
                    }
                    control_center::MaybeEndpointController::Busy(EndpointControllerQueue {
                        inner: queue,
                        endpoints,
                    }) => {
                        self.spawn_endpoint_controller_queue_waiter(queue);

                        let endpoints = endpoints.into_iter().map(Into::into).collect();
                        Ok(actions::Response::ControlQueue(endpoints))
                    }
                }
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    async fn control(&mut self, id: InternalEndpointId) -> ResponseResult {
        let response = self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Control(id))
            .await;

        self.handle_control_response(response).await
    }

    async fn control_any(&mut self, label: Label) -> ResponseResult {
        let response = self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::ControlAny(label))
            .await;

        self.handle_control_response(response).await
    }

    fn id_to_internal(&self, endpoint: EndpointId) -> InternalEndpointId {
        match endpoint {
            EndpointId::Tty(tty) => InternalEndpointId::Tty(tty),
            EndpointId::Mock(mock) => InternalEndpointId::Mock(self.mock_id(&mock)),
        }
    }

    async fn write(&mut self, endpoint: EndpointId, message: String) -> ResponseResult {
        let user_id = endpoint.clone();
        let id = self.id_to_internal(endpoint);

        let sender = match self
            .controllers
            .iter_mut()
            .flat_map(|controller| controller.endpoints.iter_mut())
            .find(|(id_, _)| id_ == &&id)
            .map(|(_, sender)| sender)
        {
            Some(sender) => Ok(sender),
            None => Err(error::Error::NoPermit(format!("write {user_id}"))),
        }?;

        sender
            .send(message)
            .await
            .expect("Endpoint should be alive");
        Ok(actions::Response::Ok)
    }

    #[async_recursion]
    async fn do_user_action(&mut self, action: actions::Action) -> ResponseResult {
        info!("client requested action: {action}");

        // TODO: Collapse these
        match action {
            actions::Action::Observe(id @ EndpointId::Tty(_)) => {
                self.observe(self.id_to_internal(id)).await
            }
            actions::Action::Observe(id @ EndpointId::Mock(_)) => {
                self.observe(self.id_to_internal(id)).await
            }
            actions::Action::Control(id @ EndpointId::Tty(_)) => {
                self.control(self.id_to_internal(id)).await
            }
            actions::Action::Control(id @ EndpointId::Mock(_)) => {
                self.control(self.id_to_internal(id)).await
            }
            actions::Action::ControlAny(label) => self.control_any(label).await,
            actions::Action::Write((endpoint, message)) => self.write(endpoint, message).await,
        }
    }
}
