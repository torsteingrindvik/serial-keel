use std::net::SocketAddr;
use tokio::sync::mpsc;

use futures::{sink::Sink, SinkExt, StreamExt};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, WebSocketUpgrade,
    },
    response::IntoResponse,
    Extension, TypedHeader,
};

use futures::stream::Stream;

use tracing::{debug, info, trace};

use crate::{
    actions::{self, ResponseResult},
    control_center::{Action, ControlCenterHandle},
    error, peer,
};

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(cc_handle): Extension<ControlCenterHandle>,
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        info!("`{}`@`{addr}` connected", user_agent.as_str());
    }

    ws.on_upgrade(move |socket| handle_websocket(socket, addr, cc_handle))
}

fn deserialize_user_request(request_text: &str) -> Result<actions::Action, error::Error> {
    serde_json::from_str::<'_, actions::Action>(request_text)
        .map_err(|e| error::Error::BadRequest(format!("Request: `{request_text:?}`, error: {e:?}")))
}

pub(crate) async fn read<S>(
    mut receiver: S,
    sender: mpsc::UnboundedSender<ResponseResult>,
    peer_handle: peer::PeerHandle,
    cc_handle: ControlCenterHandle,
) where
    S: Unpin,
    S: Stream<Item = Result<Message, axum::Error>>,
{
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(request_text) => {
                trace!(%request_text, "peer request");
                match serde_json::from_str(&request_text) {
                    Ok(request) => peer_handle.send(request),
                    Err(e) => {
                        sender
                            .send(Err(error::Error::BadRequest(format!(
                        "Request `{request_text}` is not a valid JSON formatted user action"
                    ))))
                            .expect("Sender should be alive");
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

    peer_handle.shutdown().await;

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
        debug!("Reply flushed");
    }
}

pub(crate) async fn handle_websocket(
    websocket: WebSocket,
    socket_addr: SocketAddr,
    cc_handle: ControlCenterHandle,
) {
    let (stream_sender, stream_receiver) = websocket.split();

    let (response_sender, response_receiver) = mpsc::unbounded_channel::<ResponseResult>();

    let peer = peer::Peer::new(todo!());

    let read_handle = tokio::spawn(read(stream_receiver, response_sender, cc_handle));
    let write_handle = tokio::spawn(write(stream_sender, response_receiver));

    match read_handle.await {
        Ok(()) => info!("Read task joined"),
        Err(e) => info!("Read task join error: {e:?}"),
    }

    debug!("Aborting write task");
    // This ensures the underlying TCP connection gets closed,
    // which signals the peer that the session is over.
    write_handle.abort();
}
