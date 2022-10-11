//! The Control Center has control over the endpoints.
//! For example, it is able to create mock endpoints.
//! Also, when someone needs to observe an endpoint, the
//! control center is able to provide the channel to observe.

use std::collections::HashMap;

use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    actions::{self, Action},
    endpoint::{Endpoint, EndpointLabel},
    error::Error,
};

pub(crate) struct ControlCenter {
    endpoints: HashMap<EndpointLabel, Box<dyn Endpoint + Send>>,

    inbox: mpsc::UnboundedReceiver<ControlCenterRequest>,
    outbox: mpsc::UnboundedSender<ControlCenterRequest>,
}

pub(crate) struct ControlCenterRequest {
    action: actions::Action,
    response: oneshot::Sender<Result<ControlCenterResponse, Error>>,
}

pub(crate) enum ControlCenterResponse {
    ObserveThis(broadcast::Receiver<SerialMessage>),
}

impl ControlCenter {
    pub(crate) fn new() -> Self {
        let (outbox, inbox) = mpsc::unbounded_channel();

        Self {
            endpoints: HashMap::new(),
            inbox,
            outbox,
        }
    }

    pub(crate) fn run(mut self) {
        tokio::spawn(async move {
            while let Some(request) = self.inbox.recv().await {
                let response = match request.action {
                    Action::Observe(label) => match self.endpoints.get(&label) {
                        Some(endpoint) => Ok(ControlCenterResponse::ObserveThis(endpoint.inbox())),
                        None => Err(Error::BadRequest("No such label".into())),
                    },
                    Action::CreateMockEndpoint {
                        name,
                        mocked_output,
                    } => todo!(),
                };

                request.response.send(response);
            }
            unreachable!("ControlCenter run over not possible- we own a sender so there will always be at least one alive");
        });
    }

    pub(crate) fn handle(&self) -> mpsc::UnboundedSender<ControlCenterRequest> {
        self.outbox.clone()
    }
}
