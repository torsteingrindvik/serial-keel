use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::debug;
use tracing_subscriber::prelude::*;

use crate::{control_center::ControlCenter, websocket};

async fn run(port: Option<u16>, allocated_port: Option<oneshot::Sender<u16>>) {
    static TRACING_IS_INITIALIZED: Mutex<bool> = Mutex::new(false);

    {
        let mut initialized = TRACING_IS_INITIALIZED.lock().unwrap();

        if !*initialized {
            tracing_subscriber::registry()
                .with(tracing_subscriber::EnvFilter::new(
                    std::env::var("RUST_LOG")
                        .unwrap_or_else(|_| "example_websockets=debug,tower_http=debug".into()),
                ))
                .with(tracing_subscriber::fmt::layer())
                .init();
            *initialized = true;
        }
    }

    // TODO: Clonable, state for each ws
    let control_center = ControlCenter::run();

    let app = Router::new()
        .route("/ws", get(websocket::ws_handler))
        // logging so we can see whats going on
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(0)));
    let server = axum::Server::bind(&addr).serve(app.into_make_service());
    let addr = server.local_addr();

    if let Some(port_reply) = allocated_port {
        port_reply
            .send(addr.port())
            .expect("The receiver of which port was allocated should not be dropped");
    }

    debug!("listening on {}", addr);

    server.await.unwrap();
}

/// Start the server on an arbitrary available port.
/// The port allocated will be sent on the provided channel.
pub async fn run_any_port(allocated_port: oneshot::Sender<u16>) {
    run(None, Some(allocated_port)).await
}

/// Start the server on the given port.
pub async fn run_on_port(port: u16) {
    run(Some(port), None).await
}

// mod websocket {
//     use tokio::sync::oneshot::tokio;

//     use tokio::sync::oneshot;

//     use tokio::sync::mpsc;

//     use tokio::sync::mpsc::UnboundedReceiver;

//     use futures::sink::Sink;

//     use axum;

//     use axum::extract::ws::Message;

//     use futures::stream::Stream;

//     use tokio::sync::mpsc::UnboundedSender;

//     use tracing::debug;

//     use axum::response::IntoResponse;

//     use axum::TypedHeader;

//     use axum::extract::ws::WebSocketUpgrade;

//     pub(crate) async fn ws_handler(
//         ws: WebSocketUpgrade,
//         user_agent: Option<TypedHeader<headers::UserAgent>>,
//     ) -> impl IntoResponse {
//         if let Some(TypedHeader(user_agent)) = user_agent {
//             debug!("`{}` connected", user_agent.as_str());
//         }

//         ws.on_upgrade(handle_sink_stream)
//     }

//     pub(crate) async fn read<S>(mut receiver: S, sender: UnboundedSender<ActionResponse>)
//     where
//         S: Unpin,
//         S: Stream<Item = Result<Message, axum::Error>>,
//     {
//         while let Some(Ok(msg)) = receiver.next().await {
//             match msg {
//                 Message::Text(request_text) => {
//                     debug!("client sent str: {:?}", request_text);

//                     match serde_json::from_str::<'_, Action>(&request_text) {
//                         Ok(action) => {
//                             debug!("client requested action: {action:?}");
//                             sender.send(Response::Ok.into()).unwrap();
//                         }
//                         Err(e) => {
//                             debug!("client bad request: {e:?}");
//                             sender.send(Error::BadRequest(request_text).into()).unwrap();
//                         }
//                     }
//                 }
//                 Message::Binary(_) => {
//                     debug!("client sent binary data");
//                 }
//                 Message::Ping(_) => {
//                     debug!("socket ping");
//                 }
//                 Message::Pong(_) => {
//                     debug!("socket pong");
//                 }
//                 Message::Close(_) => {
//                     debug!("client disconnected");
//                 }
//             }
//         }
//         debug!("no more stuff");
//     }

//     pub(crate) async fn write(
//         mut sender: impl Sink<Message> + Unpin,
//         mut receiver: UnboundedReceiver<ActionResponse>,
//     ) {
//         while let Some(action_response) = receiver.recv().await {
//             debug!("Got a {action_response:?}, will reply");

//             let response: ResponseResult = action_response
//                 .try_into()
//                 .expect("Should only send responses to clients, not actions");

//             let response = serde_json::to_string(&response).expect("Serialize should work");

//             if sender.send(Message::Text(response)).await.is_err() {
//                 debug!("client disconnected");
//                 return;
//             }
//         }
//     }

//     pub(crate) async fn handle_sink_stream<S>(stream: S)
//     where
//         S: Stream<Item = Result<Message, axum::Error>>,
//         S: Sink<Message>,
//         S: Send,
//         S: 'static,
//     {
//         let (stream_sender, stream_receiver) = stream.split();

//         let (mpsc_sender, mpsc_receiver) = mpsc::unbounded_channel::<ActionResponse>();

//         let (mpsc_sender, mpsc_receiver) = mpsc::unbounded_channel::<(Action, oneshot::)>();

//         tokio::spawn(write(stream_sender, mpsc_receiver));
//         tokio::spawn(read(stream_receiver, mpsc_sender));
//     }
// }
