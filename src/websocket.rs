use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use futures::{sink::Sink, SinkExt, StreamExt};

use axum::{
    extract::{ws::Message, WebSocketUpgrade},
    response::IntoResponse,
    Extension, TypedHeader,
};

use futures::stream::Stream;

use tracing::debug;

use crate::{
    actions::{Action, ActionResponse, Response, ResponseResult},
    control_center::ControlCenterHandle,
    error::Error,
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

pub(crate) async fn read<S>(
    mut receiver: S,
    sender: UnboundedSender<ActionResponse>,
    cc_handle: ControlCenterHandle,
) where
    S: Unpin,
    S: Stream<Item = Result<Message, axum::Error>>,
{
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(request_text) => {
                debug!("client sent str: {:?}", request_text);

                match serde_json::from_str::<'_, Action>(&request_text) {
                    Ok(action) => {
                        debug!("client requested action: {action:?}");
                        sender.send(Response::Ok.into()).unwrap();
                    }
                    Err(e) => {
                        debug!("client bad request: {e:?}");
                        sender.send(Error::BadRequest(request_text).into()).unwrap();
                    }
                }
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
    mut receiver: UnboundedReceiver<ActionResponse>,
    cc_handle: ControlCenterHandle,
) {
    while let Some(action_response) = receiver.recv().await {
        debug!("Got a {action_response:?}, will reply");

        let response: ResponseResult = action_response
            .try_into()
            .expect("Should only send responses to clients, not actions");

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

    let (mpsc_sender, mpsc_receiver) = mpsc::unbounded_channel::<ActionResponse>();

    tokio::spawn(write(stream_sender, mpsc_receiver, cc_handle.clone()));
    tokio::spawn(read(stream_receiver, mpsc_sender, cc_handle));
}
