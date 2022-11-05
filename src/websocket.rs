use opentelemetry_api::trace::FutureExt;
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

// use opentelemetry_api::trace::context::FutureExt;
use tracing::{debug, info, info_span, trace, warn, Instrument, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::{
    actions::ResponseResult, control_center::ControlCenterHandle, error, peer, user::User,
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

    ws.on_upgrade(move |socket| {
        let user = User::new(&addr.to_string());

        let span = info_span!("User", %user);

        handle_websocket(socket, user, cc_handle)
            .with_context(span.context())
            .instrument(span)
    })
}

pub(crate) async fn read<S>(
    mut receiver: S,
    sender: mpsc::UnboundedSender<ResponseResult>,
    peer_handle: peer::PeerHandle,
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
                            .send(Err(error::Error::BadJson {
                                request: request_text,
                                problem: e.to_string(),
                            }))
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

    // Async drop?
    peer_handle.shutdown().await;

    debug!("no more stuff");
}

pub(crate) async fn write(
    mut sender: impl Sink<Message> + Unpin,
    mut receiver: mpsc::UnboundedReceiver<ResponseResult>,
) {
    while let Some(response) = receiver.recv().await {
        match &response {
            Ok(r) => debug!("Response: <{r}>"),
            Err(e) => info!("Error response: <{e}>"),
        }

        let response = serde_json::to_string(&response).expect("Serialize should work");

        if sender.send(Message::Text(response)).await.is_err() {
            debug!("client disconnected");
            return;
        }
        trace!("Reply flushed");
    }
}

pub(crate) async fn handle_websocket(
    websocket: WebSocket,
    user: User,
    cc_handle: ControlCenterHandle,
) {
    let (stream_sender, stream_receiver) = websocket.split();
    let (response_sender, response_receiver) = mpsc::unbounded_channel::<ResponseResult>();

    let span = info_span!("User", %user);

    let peer_handle = peer::PeerHandle::new(
        user,
        response_sender.clone(),
        cc_handle,
        info_span!(parent: &span, "Peer"),
    );

    let read_handle = tokio::spawn(
        read(stream_receiver, response_sender, peer_handle)
            .instrument(info_span!(parent: &span, "Read")),
    );
    let write_handle = tokio::spawn(
        write(stream_sender, response_receiver).instrument(info_span!(parent: &span, "Write")),
    );
    drop(span);

    match read_handle.await {
        Ok(()) => debug!("Read task joined"),
        Err(e) => warn!("Read task join error: {e:?}"),
    }

    debug!("Aborting write task");
    // This ensures the underlying TCP connection gets closed,
    // which signals the peer that the session is over.
    write_handle.abort();
}
