use std::collections::HashSet;

use tokio::sync::{broadcast, mpsc};

use futures::{sink::Sink, SinkExt, StreamExt};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        WebSocketUpgrade,
    },
    response::IntoResponse,
    Extension, TypedHeader,
};

use futures::stream::Stream;

use tracing::{debug, info, trace};

use crate::{
    actions::{self, Response, ResponseResult},
    control_center::{Action, ControlCenterHandle, ControlCenterResponse},
    endpoint::{mock::MockId, InternalEndpointLabel},
    error::Error,
    user::User,
};

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    Extension(cc_handle): Extension<ControlCenterHandle>,
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        debug!("`{}` connected", user_agent.as_str());
    }

    ws.on_upgrade(|socket| handle_websocket(socket, cc_handle))
}

async fn endpoint_handler(
    label: InternalEndpointLabel,
    mut endpoint_messages: broadcast::Receiver<String>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
    info!("Starting handler for {label}");

    while let Ok(message) = endpoint_messages.recv().await {
        if user_sender
            .send(Ok(Response::Message {
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

fn deserialize_user_request(request_text: &str) -> Result<actions::Action, Error> {
    serde_json::from_str::<'_, actions::Action>(request_text)
        .map_err(|e| Error::BadRequest(format!("Request: `{request_text:?}`, error: {e:?}")))
}

struct Peer {
    user: User,
    sender: mpsc::UnboundedSender<ResponseResult>,
    cc_handle: ControlCenterHandle,
    mocks_created: HashSet<MockId>,
}

impl Peer {
    async fn handle_request(&mut self, request_text: String) -> ResponseResult {
        debug!("client sent str: {:?}", request_text);

        let action = deserialize_user_request(&request_text)?;
        self.do_user_action(action).await
    }

    async fn do_user_action(&mut self, action: actions::Action) -> ResponseResult {
        debug!("client requested action: {action:?}");

        let action = Action::from_user_action(&self.user, action);

        // If the user is trying to observe a mock,
        // create the endpoint for them.
        let mock_id = if let Action::Observe(InternalEndpointLabel::Mock(mock_id)) = &action {
            if self.mocks_created.contains(mock_id) {
                return Err(Error::BadRequest(format!(
                    "Alreadying observing this endpoint {mock_id}"
                )));
            } else {
                self.cc_handle
                    .perform_action(Action::CreateMockEndpoint(mock_id.clone()))
                    .await
                    .expect("Should be able to create new mock endpoint");
            }
            Some(mock_id.clone())
        } else {
            None
        };

        match self.cc_handle.perform_action(action).await {
            Ok(ControlCenterResponse::Ok) => Ok(Response::Ok),
            Ok(ControlCenterResponse::ObserveThis((label, endpoint))) => {
                tokio::spawn(endpoint_handler(label, endpoint, self.sender.clone()));

                // We intended to create a mock endpoint,
                // and we succeeded.
                if let Some(mock_id) = mock_id {
                    self.mocks_created.insert(mock_id);
                }

                Ok(Response::Ok)
            }
            Err(e) => Err(e),
        }
    }
}

pub(crate) async fn read<S>(
    mut receiver: S,
    sender: mpsc::UnboundedSender<ResponseResult>,
    cc_handle: ControlCenterHandle,
) where
    S: Unpin,
    S: Stream<Item = Result<Message, axum::Error>>,
{
    let mut peer = Peer {
        sender: sender.clone(),
        cc_handle: cc_handle.clone(),
        user: User::new("hello"),
        mocks_created: HashSet::new(),
    };

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(request_text) => {
                trace!(%request_text, "peer request");
                let response = peer.handle_request(request_text).await;
                sender.send(response).unwrap();
            }
            Message::Binary(_) => {
                debug!("client sent binary data");
            }
            Message::Ping(_) => {
                debug!("socket ping");
            }
            Message::Pong(_) => {
                debug!("socket pong");
            }
            Message::Close(_) => {
                debug!("client disconnected");
            }
        }
    }

    for mock in peer.mocks_created {
        info!("Removing {mock}");
        cc_handle
            .perform_action(Action::RemoveMockEndpoint(mock))
            .await
            .expect("Should be able to remove peer mock");
    }

    debug!("no more stuff");
}

pub(crate) async fn write(
    mut sender: impl Sink<Message> + Unpin,
    mut receiver: mpsc::UnboundedReceiver<ResponseResult>,
) {
    while let Some(response) = receiver.recv().await {
        debug!("Got a {response:?}, will reply");

        let response = serde_json::to_string(&response).expect("Serialize should work");

        if sender.send(Message::Text(response)).await.is_err() {
            debug!("client disconnected");
            return;
        }
    }
}

pub(crate) async fn handle_websocket(websocket: WebSocket, cc_handle: ControlCenterHandle) {
    let (stream_sender, stream_receiver) = websocket.split();

    let (response_sender, response_receiver) = mpsc::unbounded_channel::<ResponseResult>();

    let read_handle = tokio::spawn(read(stream_receiver, response_sender, cc_handle));
    let write_handle = tokio::spawn(write(stream_sender, response_receiver));

    match read_handle.await {
        Ok(()) => info!("Read task joined"),
        Err(e) => info!("Read task join error: {e:?}"),
    }

    info!("Aborting write task");
    // This ensures the underlying TCP connection gets closed,
    // which signals the peer that the session is over.
    write_handle.abort();
}
