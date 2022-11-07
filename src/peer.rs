use async_recursion::async_recursion;
use futures::SinkExt;
use std::collections::{HashMap, HashSet};

use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tracing::{debug, info, info_span, warn, Instrument};

use crate::{
    actions::{self, ResponseResult},
    control_center::{self, ControlCenterHandle},
    endpoint::{EndpointLabel, InternalEndpointLabel, MaybeOutbox, Outbox},
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
    mocks_observing: HashSet<MockId>,

    // Which outboxes the peer may send messages to.
    // These outboxes contain permits, which grants
    // exclusive access.
    outboxes: HashMap<EndpointLabel, Outbox>,

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
    OutboxReady {
        outbox: Outbox,
        endpoint_label: EndpointLabel,
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
            mocks_observing: HashSet::new(),
            outboxes: HashMap::new(),
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
                PeerRequest::InternalAction(PeerAction::OutboxReady {
                    outbox,
                    endpoint_label,
                }) => {
                    info!(%endpoint_label, "Control granted");
                    assert!(self
                        .outboxes
                        .insert(endpoint_label.clone(), outbox)
                        .is_none());
                    self.sender
                        .send(Ok(actions::Response::ControlGranted(endpoint_label)))
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

    async fn control(&mut self, label: InternalEndpointLabel) -> ResponseResult {
        match self
            .cc_handle
            .perform_action(self.user.clone(), control_center::Action::Control(label))
            .await
        {
            Ok(control_center::ControlCenterResponse::ControlThis((label, maybe_outbox))) => {
                match maybe_outbox {
                    MaybeOutbox::Available(outbox) => {
                        assert!(self.outboxes.insert(label.clone().into(), outbox).is_none());
                        Ok(actions::Response::ControlGranted(label.into()))
                    }
                    MaybeOutbox::Busy(queue) => {
                        let outbox_sender = self.peer_requests_sender.clone();

                        // The outbox is already taken.
                        // We have to wait for it and then receive it.
                        let task_label = label.clone();
                        tokio::spawn(
                            async move {
                                match queue.0.await {
                                    Ok(outbox) => {
                                        debug!("Outbox received after queue");
                                        if outbox_sender
                                            .send(PeerRequest::InternalAction(
                                                PeerAction::OutboxReady {
                                                    outbox,
                                                    endpoint_label: task_label.into(),
                                                },
                                            ))
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

                        Ok(actions::Response::ControlQueue(label.into()))
                    }
                }
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    async fn write(&mut self, endpoint: EndpointLabel, message: String) -> ResponseResult {
        let outbox = match self.outboxes.get_mut(&endpoint) {
            Some(outbox) => Ok(outbox),
            None => Err(error::Error::NoPermit(format!("write {endpoint:?}"))),
        }?;

        outbox
            .inner
            .send(message)
            .await
            .expect("Endpoint should be alive");
        Ok(actions::Response::Ok)
    }

    #[async_recursion]
    async fn do_user_action(&mut self, action: actions::Action) -> ResponseResult {
        info!("client requested action: {action}");

        match action {
            actions::Action::Observe(EndpointLabel::Tty(tty)) => {
                self.observe(InternalEndpointLabel::Tty(tty)).await
            }
            actions::Action::Observe(EndpointLabel::Mock(endpoint)) => {
                let mock_id = self.mock_id(&endpoint);
                let response = self
                    .observe(InternalEndpointLabel::Mock(mock_id.clone()))
                    .await;

                if response.is_ok() && !self.mocks_observing.insert(mock_id) {
                    warn!("Mock already observed by this user");
                }

                response
            }
            actions::Action::Control(EndpointLabel::Tty(tty)) => {
                self.control(InternalEndpointLabel::Tty(tty)).await
            }
            actions::Action::Control(EndpointLabel::Mock(endpoint)) => {
                self.control(InternalEndpointLabel::Mock(self.mock_id(&endpoint)))
                    .await
            }
            actions::Action::Write((endpoint, message)) => self.write(endpoint, message).await,
        }
    }
}
