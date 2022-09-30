use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Router, TypedHeader};
use futures::sink::Sink;
use futures::stream::Stream;
use futures::{sink::SinkExt, stream::StreamExt};
use std::net::SocketAddr;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing_subscriber::prelude::*;

async fn run(port: Option<u16>) {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "example_websockets=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/ws", get(ws_handler))
        // logging so we can see whats going on
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port.unwrap_or(0)));
    let server = axum::Server::bind(&addr).serve(app.into_make_service());
    let addr = server.local_addr();
    tracing::debug!("listening on {}", addr);

    server.await.unwrap();
}

async fn run_any_port() {
    run(None).await
}

async fn run_on_port(port: u16) {
    run(Some(port)).await
}

#[tokio::main]
async fn main() {
    run_on_port(3000).await
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        println!("`{}` connected", user_agent.as_str());
    }

    ws.on_upgrade(handle_sink_stream)
}

async fn read<S>(mut receiver: S, sender: UnboundedSender<()>)
where
    S: Unpin,
    S: Stream<Item = Result<Message, axum::Error>>,
{
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(t) => {
                println!("client sent str: {:?}", t);

                sender.send(()).unwrap();
            }
            Message::Binary(_) => {
                println!("client sent binary data");
            }
            Message::Ping(_) => {
                println!("socket ping");
            }
            Message::Pong(_) => {
                println!("socket pong");
            }
            Message::Close(_) => {
                println!("client disconnected");
            }
        }
    }
    println!("no more stuff");
}

// async fn write(mut sender: SplitSink<WebSocket, Message>, mut receiver: UnboundedReceiver<()>) {
async fn write(mut sender: impl Sink<Message> + Unpin, mut receiver: UnboundedReceiver<()>) {
    while let Some(()) = receiver.recv().await {
        println!("Got a (), will reply");
        if sender
            .send(Message::Text(String::from("Hi!")))
            .await
            .is_err()
        {
            println!("client disconnected");
            return;
        }
    }
}

async fn handle_sink_stream<S>(stream: S)
where
    S: Stream<Item = Result<Message, axum::Error>>,
    S: Sink<Message>,
    S: Send,
    S: 'static,
{
    let (stream_sender, stream_receiver) = stream.split();
    let (mpsc_sender, mpsc_receiver) = mpsc::unbounded_channel::<()>();

    tokio::spawn(write(stream_sender, mpsc_receiver));
    tokio::spawn(read(stream_receiver, mpsc_sender));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn can_connect() {
        tokio::spawn(async move { run_any_port().await });

        tokio_tungstenite::connect_async("ws://localhost:3000/ws")
            .await
            .unwrap();
    }
}
