use std::{
    collections::HashMap,
    fmt::Display,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{channel::mpsc, executor::block_on, Sink, SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info};

use crate::{
    actions::{self, Action, Async, Response, ResponseResult},
    endpoint::LabelledEndpointId,
    error::Error,
    serial::{SerialMessage, SerialMessageBytes},
};

/// todo
pub struct ClientHandle {
    tx: ClientHandleTx,
    rx: ClientHandleRx,
}

/// A reader for an endpoint.
#[derive(Debug)]
pub struct EndpointReader {
    endpoint_id: LabelledEndpointId,

    /// Messages can be awaited here.
    messages: mpsc::UnboundedReceiver<SerialMessageBytes>,
}

impl EndpointReader {
    fn new(id: LabelledEndpointId, rx: mpsc::UnboundedReceiver<SerialMessageBytes>) -> Self {
        Self {
            endpoint_id: id,
            messages: rx,
        }
    }

    /// TODO
    pub async fn next_message(&mut self) -> SerialMessage {
        String::from_utf8_lossy(&self.messages.next().await.unwrap()).to_string()
    }

    /// Get the next message if there is one.
    pub fn try_next_message(&mut self) -> Option<SerialMessage> {
        match self.messages.try_next() {
            Ok(Some(message)) => Some(String::from_utf8_lossy(&message).to_string()),
            Ok(None) => panic!("Endpoint closed"),
            Err(_) => None,
        }
    }

    /// Borrow the [`LabelledEndpointId`].
    pub fn endpoint_id(&self) -> &LabelledEndpointId {
        &self.endpoint_id
    }
}

/// A writer for an endpoint.
#[derive(Debug)]
pub struct EndpointWriter {
    endpoint_id: LabelledEndpointId,
    messages: mpsc::UnboundedSender<Action>,
}
impl Display for EndpointWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.endpoint_id)
    }
}

impl EndpointWriter {
    fn new(id: LabelledEndpointId, messages: mpsc::UnboundedSender<Action>) -> Self {
        Self {
            endpoint_id: id,
            messages,
        }
    }

    /// Write TODO
    pub async fn write<M>(&mut self, message: M) -> Result<(), Error>
    where
        M: AsRef<[u8]>,
    {
        self.messages
            .send(Action::write_bytes(
                &self.endpoint_id.id,
                message.as_ref().into(),
            ))
            .await
            .expect("TODO");

        Ok(())
    }

    /// Borrow the [`LabelledEndpointId`].
    pub fn endpoint_id(&self) -> &LabelledEndpointId {
        &self.endpoint_id
    }
}

struct Client {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,

    /// Writes any non-async responses
    responses: mpsc::UnboundedSender<Result<ClientResponse, Error>>,

    action_requests_tx: mpsc::UnboundedSender<Action>,
    action_requests_rx: mpsc::UnboundedReceiver<Action>,

    endpoint_readers: HashMap<LabelledEndpointId, mpsc::UnboundedSender<SerialMessageBytes>>,
}

/// The response the client exposes through its API.
#[derive(Debug)]
pub enum ClientResponse {
    /// A requested write action was successful.
    WriteOk,

    /// Now observing the given endpoints.
    Observing(Vec<EndpointReader>),

    /// Now controlling the given endpoints.
    Controlling(Vec<EndpointWriter>),

    /// Queued.
    Queued,
}

impl Client {
    async fn handle_websocket_message(
        message: Option<Result<tungstenite::protocol::Message, tungstenite::Error>>,
        endpoint_readers: &mut HashMap<
            LabelledEndpointId,
            mpsc::UnboundedSender<SerialMessageBytes>,
        >,
        responses: &mut mpsc::UnboundedSender<Result<ClientResponse, Error>>,
        actions_tx: mpsc::UnboundedSender<Action>,
    ) {
        let text = match message {
            Some(Ok(tungstenite::protocol::Message::Text(text))) => text,
            Some(Err(e)) => {
                error!(?e, "Wrong thing");
                return;
            }
            Some(others) => {
                error!(?others, "Unhandled");
                return;
            }
            None => {
                error!("Wrong thing");
                return;
            }
        };

        let response: ResponseResult = match serde_json::from_str(&text) {
            Ok(response) => response,
            Err(e) => {
                error!(?e, ?text, "Could not deserialize message");
                return;
            }
        };

        let response = match response {
            Ok(response) => response,
            Err(e) => {
                if let Err(send_error) = responses.send(Err(e)).await {
                    error!(?send_error, "Could not send message to client");
                }
                return;
            }
        };

        use actions::Sync::*;
        let response = match response {
            Response::Sync(response) => match response {
                Observing(ref ids) => {
                    let mut readers = vec![];
                    for id in ids {
                        endpoint_readers.entry(id.clone()).or_insert_with(|| {
                            let (tx, rx) = mpsc::unbounded();
                            readers.push(EndpointReader::new(id.clone(), rx));

                            tx
                        });
                    }
                    ClientResponse::Observing(readers)
                }
                WriteOk => ClientResponse::WriteOk,
                ControlQueue(_) => ClientResponse::Queued,
                ControlGranted(ref ids) => {
                    let mut writers = vec![];
                    for id in ids {
                        writers.push(EndpointWriter::new(id.clone(), actions_tx.clone()));
                    }
                    ClientResponse::Controlling(writers)
                }
            },
            Response::Async(Async::Message { endpoint, message }) => {
                let tx = endpoint_readers
                    .get_mut(&endpoint)
                    .expect("We will not be sent messages of endpoints we are not observing");

                debug!("Got a message");
                tx.unbounded_send(message).expect("Should be alive");
                return;
            }
        };

        if let Err(e) = responses.send(Ok(response)).await {
            error!(?e, "Could not send message to client");
        }
    }

    async fn run(self) {
        let (mut ws_tx, mut ws_rx) = self.stream.split();

        let mut actions_rx = self.action_requests_rx;
        let mut response_tx = self.responses;

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

        let mut endpoint_readers = self.endpoint_readers;

        let response_handle = tokio::spawn(async move {
            loop {
                let actions_tx = self.action_requests_tx.clone();
                let ws_msg = ws_rx.next().await;
                Self::handle_websocket_message(
                    ws_msg,
                    &mut endpoint_readers,
                    &mut response_tx,
                    actions_tx,
                )
                .await;
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

    /// Send an [`Action`] to start observing a TTY endpoint with the given path.
    pub async fn observe_tty(&mut self, tty: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::observe_tty(tty)).await
    }

    /// Send an [`Action`] to start observing a mock endpoint with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::observe_mock(name)).await
    }

    /// Send an [`Action`] to start controlling a mock endpoint with the given name.
    pub async fn control_mock(&mut self, name: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::control_mock(name)).await
    }

    /// Send an [`Action`] to start controlling a tty endpoint with the given path.
    pub async fn control_tty(&mut self, path: &str) -> Result<(), Error> {
        self.send_or_ws_issue(Action::control_tty(path)).await
    }

    /// TODO
    pub async fn control_any<S: AsRef<str>>(&mut self, labels: &[S]) -> Result<(), Error> {
        self.send_or_ws_issue(Action::control_any(labels)).await
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
pub struct ClientHandleRx {
    responses: mpsc::UnboundedReceiver<Result<ClientResponse, Error>>,
}

impl ClientHandleRx {
    /// Await the next response from the transport.
    pub async fn next_response(&mut self) -> Result<ClientResponse, Error> {
        debug!("Waiting");
        self.responses
            .next()
            .await
            .ok_or_else(|| Error::WebsocketIssue("Next was None".into()))?
    }
}

impl ClientHandle {
    async fn new_stream(
        address: &str,
        port: u16,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        let (stream, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}:{port}/client")).await?;
        Ok(stream)
    }

    fn new_stream_blocking(
        address: &str,
        port: u16,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        // Wraps the async function in a blocking call.
        block_on(Self::new_stream(address, port))
    }

    fn new_impl(stream: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Result<Self, Error> {
        let (action_tx, action_rx) = mpsc::unbounded();
        let (response_tx, response_rx) = mpsc::unbounded();

        let client = Client {
            responses: response_tx,
            action_requests_tx: action_tx.clone(),
            action_requests_rx: action_rx,
            stream,
            endpoint_readers: HashMap::new(),
        };

        tokio::spawn(async move { client.run().await });

        Ok(Self {
            tx: ClientHandleTx(action_tx),
            rx: ClientHandleRx {
                responses: response_rx,
            },
        })
    }

    /// Create a new [`ClientHandle`] from the given address and port, connecting asynchronously.
    pub async fn new(address: &str, port: u16) -> Result<Self, Error> {
        let stream = Self::new_stream(address, port).await?;
        Self::new_impl(stream)
    }

    /// Create a new [`ClientHandle`] from the given address and port.
    pub fn new_blocking(address: &str, port: u16) -> Result<Self, Error> {
        let stream = Self::new_stream_blocking(address, port)?;
        Self::new_impl(stream)
    }

    async fn observe_response(&mut self) -> Result<Vec<EndpointReader>, Error> {
        match self.rx.next_response().await {
            Ok(ClientResponse::Observing(endpoints)) => Ok(endpoints),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    // TODO: We'd like to have something more, e.g.
    // observe(&mut self, thing: impl Into<DescribesEndpoint>).
    //
    // This way we can feed an EndpointWriter into it and use the id.

    /// Start observing the mock with the given name.
    pub async fn observe_tty(&mut self, path: &str) -> Result<Vec<EndpointReader>, Error> {
        self.tx.observe_tty(path).await?;
        self.observe_response().await
    }

    /// Start observing the mock with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<Vec<EndpointReader>, Error> {
        self.tx.observe_mock(name).await?;
        self.observe_response().await
    }

    async fn wait_for_control(&mut self) -> Result<Vec<EndpointWriter>, Error> {
        match self.rx.next_response().await {
            Ok(ClientResponse::Controlling(endpoints)) => {
                info!(?endpoints, "Granted");
                Ok(endpoints)
            }
            Ok(ClientResponse::Queued) => {
                info!("Queued");
                let after_queue = self.rx.next_response().await?;
                match after_queue {
                    ClientResponse::Controlling(endpoints) => {
                        info!(?endpoints, "Granted");
                        Ok(endpoints)
                    }
                    _ => unreachable!(),
                }
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    /// Start controlling the mock with the given name.
    pub async fn control_mock(&mut self, name: &str) -> Result<Vec<EndpointWriter>, Error> {
        self.tx.control_mock(name).await?;
        self.wait_for_control().await
    }

    /// Start controlling the mock with the given name.
    pub async fn control_tty(&mut self, path: &str) -> Result<Vec<EndpointWriter>, Error> {
        self.tx.control_tty(path).await?;
        self.wait_for_control().await
    }

    /// Start controlling any endpoint with the matching labels.
    pub async fn control_any<S: AsRef<str>>(
        &mut self,
        labels: &[S],
    ) -> Result<Vec<EndpointWriter>, Error> {
        self.tx.control_any(labels).await?;
        self.wait_for_control().await
    }
}
