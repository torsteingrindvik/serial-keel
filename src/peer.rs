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
    endpoint::{EndpointLabel, InternalEndpointLabel},
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
    // outboxes: HashMap<EndpointLabel, Outbox>,
    controllers: Vec<EndpointController>,

    // The handle to the control center,
    // which holds global state.
    cc_handle: ControlCenterHandle,
}

// TODO: Close this gracefully?
async fn endpoint_handler(
    label: InternalEndpointLabel,
    mut endpoint_messages: broadcast::Receiver<String>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
    info!("Starting handler for {label}");

    while let Ok(message) = endpoint_messages.recv().await {
        if user_sender
            .send(Ok(actions::Response::Message {
                endpoint: label.clone().into(),
                message,
            }))
            .is_err()
        {
            debug!("Send error");
            break;
        }
    }

    debug!("Endpoint `{label:?}` closed")
}

#[derive(Debug)]
pub(crate) enum PeerAction {
    /// An outbox we're waiting for is now ready.
    ControllerReady {
        controller: EndpointController,
        // endpoint_label: EndpointLabel,
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
                        .inform(control_center::Inform::UserLeft(self.user.clone()));

                    break;
                }
                PeerRequest::InternalAction(PeerAction::ControllerReady { controller }) => {
                    let granted_labels = self.add_endpoint_controller(controller);

                    self.sender
                        .send(Ok(actions::Response::ControlGranted(granted_labels)))
                        .expect("If we're alive it means the websocket connection should be up")
                }
            }
        }
    }

    async fn observe(&mut self, label: InternalEndpointLabel) -> ResponseResult {
        match self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Observe(label))
            .await
        {
            Ok(control_center::ControlCenterResponse::ObserveThis((label, endpoint))) => {
                let span = info_span!("Endpoint Handler", %label);

                tokio::spawn(
                    endpoint_handler(label, endpoint, self.sender.clone()).instrument(span),
                );

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

    fn add_endpoint_controller(&mut self, controller: EndpointController) -> Vec<EndpointLabel> {
        // assert!(self.outboxes.insert(label.clone().into(), outbox).is_none());
        let labels = controller.endpoints.keys().cloned().collect_vec();

        self.controllers.push(controller);
        // Log them in their internal representation
        info!(?labels, "Control granted");

        // Now convert to the user/external representation
        labels
            .into_iter()
            .map(|internal_label| EndpointLabel::from(internal_label))
            .collect()
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

    async fn control(&mut self, label: InternalEndpointLabel) -> ResponseResult {
        match self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Control(label))
            .await
        {
            Ok(control_center::ControlCenterResponse::ControlThis(maybe_controller)) => {
                match maybe_controller {
                    control_center::MaybeEndpointController::Available(controller) => {
                        let granted_labels = self.add_endpoint_controller(controller);
                        Ok(actions::Response::ControlGranted(granted_labels))
                    }
                    control_center::MaybeEndpointController::Busy(EndpointControllerQueue {
                        queue,
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

    fn label_to_internal(&self, endpoint: EndpointLabel) -> InternalEndpointLabel {
        match endpoint {
            EndpointLabel::Tty(tty) => InternalEndpointLabel::Tty(tty),
            EndpointLabel::Mock(mock) => InternalEndpointLabel::Mock(self.mock_id(&mock)),
        }
    }

    async fn write(&mut self, endpoint: EndpointLabel, message: String) -> ResponseResult {
        let user_label = endpoint.clone();
        let label = self.label_to_internal(endpoint);

        let sender = match self
            .controllers
            .iter_mut()
            .map(|controller| controller.endpoints.iter_mut())
            .flatten()
            .find(|(label_, _)| label_ == &&label)
            .map(|(_, sender)| sender)
        {
            Some(sender) => Ok(sender),
            None => Err(error::Error::NoPermit(format!("write {user_label}"))),
        }?;

        // let outbox = match self.outboxes.get_mut(&label) {
        //     Some(outbox) => Ok(outbox),
        //     None => Err(error::Error::NoPermit(format!("write {endpoint:?}"))),
        // }?;

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
            actions::Action::Observe(label @ EndpointLabel::Tty(_)) => {
                self.observe(self.label_to_internal(label)).await
            }
            actions::Action::Observe(label @ EndpointLabel::Mock(_)) => {
                self.observe(self.label_to_internal(label)).await

                // if response.is_ok() && !self.mocks_observing.insert(mock_id) {
                //     warn!("Mock already observed by this user");
                // }

                // response
            }
            actions::Action::Control(label @ EndpointLabel::Tty(_)) => {
                self.control(self.label_to_internal(label)).await
            }
            actions::Action::Control(label @ EndpointLabel::Mock(_)) => {
                self.control(self.label_to_internal(label)).await
            }
            actions::Action::Write((endpoint, message)) => self.write(endpoint, message).await,
        }
    }
}
