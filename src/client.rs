use std::{
    pin::Pin,
    task::{Context, Poll},
};

// use color_eyre::Report;
use futures::{channel::mpsc, Sink, SinkExt, Stream, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error};
use tungstenite::Message;

use crate::{
    actions::{Action, Response, ResponseResult},
    error::Error,
};

/// todo
pub struct ClientHandle {
    tx: ClientHandleTx,
    rx: ClientHandleRx,
}

struct Client {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    // Would actually be nice to split this up somewhat
    writer: mpsc::UnboundedSender<ResponseResult>,
    reader: mpsc::UnboundedReceiver<Action>,
}

impl Client {
    async fn run(self) {
        let (mut ws_tx, mut ws_rx) = self.stream.split();

        let mut actions_rx = self.reader;
        let mut response_tx = self.writer;

        let actions_handle = tokio::spawn(async move {
            while let Some(action) = actions_rx.next().await {
                if let Err(e) = ws_tx
                    .send(tungstenite::Message::Text(action.serialize()))
                    .await
                {
                    error!(?e, "Could not send message to server");
                    break;
                }
            }
        });

        let response_handle = tokio::spawn(async move {
            while let Some(Ok(Message::Text(text))) = ws_rx.next().await {
                let response: ResponseResult = match serde_json::from_str(&text) {
                    Ok(response) => response,
                    Err(e) => {
                        error!(?e, ?text, "Could not deserialize message");
                        break;
                    }
                };

                if let Err(e) = response_tx.send(response).await {
                    error!(?e, "Could not send message to server");
                    break;
                }
            }
        });

        // TODO: Abort the other?
        tokio::select! {
            _ = actions_handle => {
                debug!("Actions loop returned");
            },
            _ = response_handle => {
                debug!("Response loop returned");
            },
        }
    }
}

/// The clonable sender the client can use to ask actions of the server.
#[derive(Debug, Clone)]
pub struct ClientHandleTx(mpsc::UnboundedSender<Action>);

impl ClientHandleTx {
    async fn send_or_ws_issue(&mut self, action: Action) -> Result<(), Error> {
        self.send(action)
            .await
            .map_err(|e| Error::WebsocketIssue(e.to_string()))
    }

    /// Send an [`Action`] to start observing a mock endpoint with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::observe_mock(name)).await
    }

    /// Send an [`Action`] to start controlling a mock endpoint with the given name.
    pub async fn control_mock(&mut self, name: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::control_mock(name)).await
    }
}

impl Sink<Action> for ClientHandleTx {
    type Error = mpsc::SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Action) -> Result<(), Self::Error> {
        self.0.start_send_unpin(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_close_unpin(cx)
    }
}

/// The single receiver a client has for responses from the server.
#[derive(Debug)]
pub struct ClientHandleRx(mpsc::UnboundedReceiver<ResponseResult>);

impl ClientHandleRx {
    /// Await the next response from the transport.
    pub async fn next_response(&mut self) -> ResponseResult {
        self.next()
            .await
            .ok_or_else(|| Error::WebsocketIssue("Next was None".into()))?
    }
}

impl Stream for ClientHandleRx {
    type Item = ResponseResult;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx)
    }
}

impl ClientHandle {
    /// Given a port and an address on the format `{}`
    pub async fn new(address: &str, port: u16) -> Result<Self, Error> {
        let (stream, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}:{port}/ws")).await?;

        let (action_tx, action_rx) = mpsc::unbounded();
        let (response_tx, response_rx) = mpsc::unbounded();

        let client = Client {
            writer: response_tx,
            reader: action_rx,
            stream,
        };

        tokio::spawn(async move { client.run().await });

        Ok(Self {
            tx: ClientHandleTx(action_tx),
            rx: ClientHandleRx(response_rx),
        })
    }

    /// Split into sender and receiver parts of the client handle.
    pub fn split(self) -> (ClientHandleTx, ClientHandleRx) {
        (self.tx, self.rx)
    }

    /// Start observing the mock with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<(), Error> {
        self.tx.observe_mock(name).await?;
        match self.rx.next_response().await {
            Ok(Response::Ok) => Ok(()),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    /// Start controlling the mock with the given name.
    pub async fn control_mock(&mut self, name: &str) -> ResponseResult {
        self.tx.control_mock(name).await?;
        match self.rx.next_response().await {
            Ok(Response::ControlGranted(granted)) => todo!(),
            Ok(Response::ControlQueue(queue)) => todo!(),
            Ok(Response::) => todo!(),
            Err(e) => Err(e),
        }
    }
}
