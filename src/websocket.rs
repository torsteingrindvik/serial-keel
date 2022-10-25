use tokio::sync::{broadcast, mpsc};

use futures::{sink::Sink, SinkExt, StreamExt};

use axum::{
    extract::{ws::Message, WebSocketUpgrade},
    response::IntoResponse,
    Extension, TypedHeader,
};

use futures::stream::Stream;

use tracing::{debug, info};

use crate::{
    actions::{Action, Response, ResponseResult},
    control_center::{ControlCenterHandle, ControlCenterResponse},
    endpoint::InternalEndpointLabel,
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

    ws.on_upgrade(|socket| handle_sink_stream(socket, cc_handle))
}

async fn endpoint_handler(
    label: InternalEndpointLabel,
    mut endpoint_messages: broadcast::Receiver<String>,
    user_sender: mpsc::UnboundedSender<ResponseResult>,
) {
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

fn deserialize_user_request(request_text: &str) -> Result<Action, Error> {
    serde_json::from_str::<'_, Action>(request_text)
        .map_err(|e| Error::BadRequest(format!("Request: `{request_text:?}`, error: {e:?}")))
}

// async fn handle_request(
//     user_id: &str,
//     request_text: String,
//     sender: &mpsc::UnboundedSender<ResponseResult>,
//     cc_handle: &ControlCenterHandle,
// ) -> ResponseResult {
//     debug!("client sent str: {:?}", request_text);

//     let action = deserialize_user_request(&request_text)?;
//     do_user_action(action, sender, cc_handle).await
// }

struct Peer {
    user: User,
    sender: mpsc::UnboundedSender<ResponseResult>,
    cc_handle: ControlCenterHandle,
}

impl Peer {
    async fn handle_request(&self, request_text: String) -> ResponseResult {
        debug!("client sent str: {:?}", request_text);

        let action = deserialize_user_request(&request_text)?;
        self.do_user_action(action).await
    }

    async fn do_user_action(&self, action: Action) -> ResponseResult {
        debug!("client requested action: {action:?}");

        // let label = match &action {
        //     Action::Observe(EndpointLabel::Mock(name)) => {
        //         Some(EndpointLabel::Mock(format!("hi-{name}")))
        //     }
        //     Action::Observe(EndpointLabel::Tty(name)) => Some(EndpointLabel::Tty(name.clone())),
        //     _ => None,
        // };

        match self.cc_handle.perform_user_action(&self.user, action).await {
            Ok(ControlCenterResponse::Ok) => Ok(Response::Ok),
            Ok(ControlCenterResponse::ObserveThis((label, endpoint))) => {
                tokio::spawn(endpoint_handler(label, endpoint, self.sender.clone()));
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
    let peer = Peer {
        sender: sender.clone(),
        cc_handle,
        user: User::new("hello"),
    };

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(request_text) => {
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

pub(crate) async fn handle_sink_stream<S>(stream: S, cc_handle: ControlCenterHandle)
where
    S: Stream<Item = Result<Message, axum::Error>>,
    S: Sink<Message>,
    S: Send,
    S: 'static,
{
    let (stream_sender, stream_receiver) = stream.split();

    let (response_sender, response_receiver) = mpsc::unbounded_channel::<ResponseResult>();

    tokio::spawn(write(stream_sender, response_receiver));
    tokio::spawn(read(stream_receiver, response_sender, cc_handle));
}
