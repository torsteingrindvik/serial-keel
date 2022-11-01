use async_recursion::async_recursion;
use futures::SinkExt;
use std::collections::{HashMap, HashSet};

use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tracing::{debug, info};

use crate::{
    actions::{self, ResponseResult},
    control_center::{self, ControlCenterHandle},
    endpoint::{mock::MockId, EndpointLabel, InternalEndpointLabel, MaybeOutbox, Outbox},
    error,
    user::User,
};

pub(crate) struct Peer {
    // Which user does this peer represent
    user: User,

    // Where to put responses to this peer's requests
    sender: mpsc::UnboundedSender<ResponseResult>,

    // Tracks which mocks this peer has created,
    // useful for cleanup when they leave
    mocks_created: HashSet<MockId>,

    // Which outboxes the peer may send messages to.
    // These outboxes contain permits, which grants
    // exclusive access.
    outboxes: HashMap<EndpointLabel, Outbox>,

    // The handle to the control center,
    // which holds global state.
    cc_handle: ControlCenterHandle,
}

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
            info!("Send error");
            break;
        }
    }

    info!("Endpoint `{label:?}` closed")
}

#[derive(Debug)]
pub(crate) enum PeerAction {
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
        // TODO: Create mpsc for reqs, add to peer etc.
        let peer = Peer::new(user, sender, cc_handle);

        let peer_handle = tokio::spawn(async move { peer.run().await });

        Self {
            requests: todo!(),
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
        self.join_handle.await;
        debug!("Shutdown complete");
    }
}

impl Peer {
    fn new(
        user: User,
        sender: mpsc::UnboundedSender<ResponseResult>,
        cc_handle: ControlCenterHandle,
    ) -> Self {
        Self {
            user,
            sender,
            mocks_created: HashSet::new(),
            outboxes: HashMap::new(),
            cc_handle,
        }
    }

    async fn run(&mut self) {
        todo!()
    }

    async fn remove_mocks(&mut self) {
        for mock in self.mocks_created.drain() {
            debug!("Removing {mock}");
            self.cc_handle
                .perform_action(control_center::Action::RemoveMockEndpoint(mock))
                .await
                .expect("Should be able to remove peer mock");
        }
    }

    async fn start_observing_mock(&mut self, mock: &str) -> ResponseResult {
        let mock_id = self.mock_id(mock);

        match self
            .cc_handle
            .perform_action(control_center::Action::CreateMockEndpoint(mock_id.clone()))
            .await
        {
            Ok(control_center::ControlCenterResponse::Ok) => Ok(actions::Response::Ok),
            Ok(control_center::ControlCenterResponse::ObserveThis((label, endpoint))) => {
                tokio::spawn(endpoint_handler(label, endpoint, self.sender.clone()));

                assert!(self.mocks_created.insert(mock_id));

                Ok(actions::Response::Ok)
            }
            Ok(control_center::ControlCenterResponse::ControlThis(_)) => {
                unreachable!()
            }
            Err(e) => Err(e),
        }
    }

    fn mock_id(&self, mock: &str) -> MockId {
        MockId::new(&self.user.name, mock)
    }

    #[async_recursion]
    async fn do_user_action(&mut self, action: actions::Action) -> ResponseResult {
        debug!("client requested action: {action:?}");

        match action {
            actions::Action::Observe(EndpointLabel::Tty(_tty)) => {
                unimplemented!()
            }
            actions::Action::Observe(EndpointLabel::Mock(endpoint)) => {
                self.start_observing_mock(&endpoint).await
            }
            actions::Action::Control(EndpointLabel::Tty(_tty)) => {
                unimplemented!()
            }
            actions::Action::Control(EndpointLabel::Mock(endpoint)) => {
                // Controlling is also an opt-in for observing.
                self.start_observing_mock(&endpoint).await?;

                match self
                    .cc_handle
                    .perform_action(control_center::Action::Control(
                        InternalEndpointLabel::Mock(self.mock_id(&endpoint)),
                    ))
                    .await
                {
                    Ok(control_center::ControlCenterResponse::ControlThis((
                        label,
                        maybe_outbox,
                    ))) => match maybe_outbox {
                        MaybeOutbox::Available(outbox) => {
                            assert!(self.outboxes.insert(label.into(), outbox).is_none());
                            Ok(actions::Response::Ok)
                        }
                        MaybeOutbox::Busy(queue) => {
                            todo!();
                            Ok(actions::Response::Ok)
                        }
                    },
                    Ok(_) => unreachable!(),
                    Err(e) => Err(e),
                }
            }
            actions::Action::Write((endpoint, message)) => {
                let outbox = match self.outboxes.get_mut(&endpoint) {
                    Some(outbox) => Ok(outbox),
                    None => Err(error::Error::BadRequest(format!(
                        "We don't have a write permit for endpoint `{endpoint:?}`"
                    ))),
                }?;

                outbox
                    .inner
                    .send(message)
                    .await
                    .expect("Endpoint should be alive");
                Ok(actions::Response::Ok)
            }
        }
    }
}
