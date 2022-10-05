use std::collections::HashMap;

use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{
    actions::{self, Action, ActionResponse},
    endpoint::{Endpoint, EndpointLabel},
    error::Error,
};

pub(crate) struct ControlCenter {
    endpoints: HashMap<EndpointLabel, Box<dyn Endpoint<Error = Error> + Send>>,

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
                        Some(endpoint) => Ok(ControlCenterResponse::ObserveThis(
                            endpoint.handle().subscriber(),
                        )),
                        None => Err(Error::BadRequest("No such label".into())),
                    },
                    Action::CreateMockEndpoint {
                        name,
                        mocked_output,
                    } => todo!(),
                };

                request.response.send(response);
                // let action = match action {
                //     ActionResponse::Action(action) => action,
                //     ActionResponse::Response(_) => {
                //         unreachable!("Control center should only receive actions")
                //     }
                // };

                // match action {
                //     Action::Observe(label) => match self.endpoints.get(&label) {
                //         Some(_) => todo!(),
                //         None => todo!(),
                //     },
                //     Action::CreateMockEndpoint {
                //         name,
                //         mocked_output,
                //     } => todo!(),
                // }
            }
        });
    }

    pub(crate) fn handle(&self) -> mpsc::UnboundedSender<ActionResponse> {
        self.outbox.clone()
    }
}
