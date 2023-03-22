use std::{
    borrow::BorrowMut,
    collections::HashMap,
    fmt::Display,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    channel::{mpsc, oneshot},
    executor::block_on,
    stream::BoxStream,
    Sink, SinkExt, Stream, StreamExt,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::{
    actions::{self, Action, Async, Response, ResponseResult},
    endpoint::{LabelledEndpointId, Labels},
    error::Error,
    events,
    serial::{SerialMessage, SerialMessageBytes},
};

pub use chrono::{DateTime, Utc};

/// A handle to a client.
/// The client lives in a separate task.
#[derive(Debug)]
pub struct ClientHandle {
    tx: ClientHandleTx,
    rx: ClientHandleRx,

    _cancel_rx: oneshot::Receiver<()>,
}

/// A reader for user events.
#[derive(Debug)]
pub struct EventReader {
    /// Events can be awaited here.
    events: mpsc::UnboundedReceiver<events::TimestampedEvent>,
}

impl EventReader {
    /// Make a new event reader.
    pub fn new(events: mpsc::UnboundedReceiver<events::TimestampedEvent>) -> Self {
        Self { events }
    }

    /// Await the next er event.
    pub async fn next_event(&mut self) -> events::TimestampedEvent {
        debug!("Awaiting next event");
        self.events
            .next()
            .await
            .expect("The sender is bound to the client and should never drop")
    }

    /// Get the next event if there is one.
    pub fn try_next_event(&mut self) -> Option<events::TimestampedEvent> {
        match self.events.try_next() {
            Ok(Some(event)) => Some(event),
            Ok(None) => panic!("Endpoint closed"),
            Err(_) => None,
        }
    }

    /// TODO
    pub fn box_stream(&mut self) -> BoxStream<events::TimestampedEvent> {
        self.events.borrow_mut().boxed()
    }
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

    /// Await the next message from the endpoint.
    pub async fn next_message(&mut self) -> SerialMessage {
        String::from_utf8_lossy(&self.messages.next().await.unwrap()).into()
    }

    /// Get the next message if there is one.
    pub fn try_next_message(&mut self) -> Option<SerialMessage> {
        match self.messages.try_next() {
            Ok(Some(message)) => Some(String::from_utf8_lossy(&message).into()),
            Ok(None) => panic!("Endpoint closed"),
            Err(_) => None,
        }
    }

    /// Borrow the [`LabelledEndpointId`].
    pub fn endpoint_id(&self) -> &LabelledEndpointId {
        &self.endpoint_id
    }

    /// Use the reader exclusively as a stream of messages.
    pub fn stream(&mut self) -> impl Stream<Item = SerialMessageBytes> + '_ {
        &mut self.messages
    }

    /// Turn the reader into a stream of messages.
    pub fn into_stream(self) -> impl Stream<Item = SerialMessageBytes> {
        self.messages
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

    /// Write a message to this endpoint.
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
            .map_err(|_| Error::BadUsage("Cannot write when the client has closed".into()))
    }

    /// Borrow the [`LabelledEndpointId`].
    pub fn endpoint_id(&self) -> &LabelledEndpointId {
        &self.endpoint_id
    }

    /// If this writer refers to a TTY, get a reference to it.
    pub fn tty(&self) -> Option<&str> {
        self.endpoint_id().id.as_tty().map(|s| s.as_str())
    }

    /// Use the writer as a sink.
    pub fn sink(&self) -> impl Sink<Action> + '_ {
        &self.messages
    }

    /// Consume the writer as a sink.
    pub fn into_sink(self) -> impl Sink<Action> {
        self.messages
    }
}

/// A collection of [`EndpointWriter`].
#[derive(Debug)]
pub struct EndpointWriters(pub Vec<EndpointWriter>);

impl Display for EndpointWriters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ew in &self.0 {
            write!(f, "{} ", ew.endpoint_id)?;
        }
        Ok(())
    }
}

impl From<Vec<EndpointWriter>> for EndpointWriters {
    fn from(ews: Vec<EndpointWriter>) -> Self {
        Self(ews)
    }
}

impl EndpointWriters {
    /// Remove the given [`EndpointWriter`] from self if an endpoint with a (super)set of the given labels is owned by us.
    pub fn remove_writer_with_labels(&mut self, labels: &Labels) -> Option<EndpointWriter> {
        if let Some(index) = self
            .0
            .iter()
            .position(|ew| ew.endpoint_id.labels.is_superset(labels))
        {
            Some(self.0.remove(index))
        } else {
            None
        }
    }
}

struct Client {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,

    /// Writes any non-async responses
    responses: mpsc::UnboundedSender<Result<ClientResponse, Error>>,

    action_requests_tx: mpsc::UnboundedSender<Action>,
    action_requests_rx: mpsc::UnboundedReceiver<Action>,

    endpoint_readers: HashMap<LabelledEndpointId, mpsc::UnboundedSender<SerialMessageBytes>>,

    events_tx: mpsc::UnboundedSender<events::TimestampedEvent>,

    // Owned by the client struct, unless the handler has spawned.
    // In that casses, it is owned by the handler.
    events_rx: Option<mpsc::UnboundedReceiver<events::TimestampedEvent>>,

    close: oneshot::Sender<()>,
}

/// The response the client exposes through its API.
#[derive(Debug)]
pub enum ClientResponse {
    /// A requested write action was successful.
    WriteOk,

    /// Now receiving events from the server.
    Events(EventReader),

    /// Now observing the given endpoints.
    Observing(EndpointReader),

    /// Now controlling the given endpoints.
    Controlling(Vec<EndpointWriter>),

    /// Queued.
    Queued,
}

impl Client {
    async fn handle_websocket_message(
        message: Result<tungstenite::protocol::Message, tungstenite::Error>,
        endpoint_readers: &mut HashMap<
            LabelledEndpointId,
            mpsc::UnboundedSender<SerialMessageBytes>,
        >,
        responses: &mut mpsc::UnboundedSender<Result<ClientResponse, Error>>,
        actions_tx: mpsc::UnboundedSender<Action>,
        events_tx: &mut mpsc::UnboundedSender<events::TimestampedEvent>,
        events_rx: &mut Option<mpsc::UnboundedReceiver<events::TimestampedEvent>>,
    ) {
        let text = match message {
            Ok(tungstenite::protocol::Message::Text(text)) => text,
            Err(e) => {
                error!(?e, "Wrong thing");
                return;
            }
            others => {
                error!(?others, "Unhandled");
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
                Observing(id) => {
                    let (tx, rx) = mpsc::unbounded();

                    let reader = EndpointReader::new(id.clone(), rx);
                    if let Some(_already_exists) = endpoint_readers.insert(id.clone(), tx) {
                        panic!("Bug! Endpoint {id} already observing");
                    }

                    ClientResponse::Observing(reader)
                }
                WriteOk => ClientResponse::WriteOk,
                ObservingEvents => ClientResponse::Events(EventReader::new(
                    events_rx
                        .take()
                        .expect("Should be able to take the events receiver"),
                )),
                ControlQueue(_) => ClientResponse::Queued,
                ControlGranted(ref ids) => {
                    let mut writers = vec![];
                    for id in ids {
                        writers.push(EndpointWriter::new(id.clone(), actions_tx.clone()));
                    }
                    ClientResponse::Controlling(writers)
                }
            },
            Response::Async(Async::Event(user_event)) => {
                debug!(?user_event, "Async response");
                events_tx
                    .unbounded_send(user_event)
                    .expect("Should be alive");
                return;
            }
            Response::Async(Async::Message { endpoint, message }) => {
                let tx = endpoint_readers
                    .get_mut(&endpoint)
                    .expect("We will not be sent messages of endpoints we are not observing");

                debug!("Got a message");
                if tx.unbounded_send(message).is_err() {
                    warn!(%endpoint, "Could not forward message to user `EventReader`- likely they have dropped it.");
                }
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
        let mut user_events_tx = self.events_tx;
        let mut user_events_rx = self.events_rx;

        let response_handle = tokio::spawn(async move {
            loop {
                let actions_tx = self.action_requests_tx.clone();
                let Some(ws_msg) = ws_rx.next().await else {
                    error!("The websocket stream closed (next message was `None`). The connection is likely broken. We cannot handle this- closing client.");
                    break;
                };
                Self::handle_websocket_message(
                    ws_msg,
                    &mut endpoint_readers,
                    &mut response_tx,
                    actions_tx,
                    &mut user_events_tx,
                    &mut user_events_rx,
                )
                .await;
            }
        });
        let mut close = self.close;

        close.cancellation().await;
        response_handle.abort();
        actions_handle.abort();
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

    /// Control any endpoint matching all the given labels.
    pub async fn control_any<S: AsRef<str>>(&mut self, labels: &[S]) -> Result<(), Error> {
        self.send_or_ws_issue(Action::control_any(labels)).await
    }

    /// Start observing events.
    pub async fn observe_events(&mut self) -> Result<(), Error> {
        self.send_or_ws_issue(Action::ObserveEvents).await
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
        let (user_events_tx, user_events_rx) = mpsc::unbounded();

        let (cancel_tx, cancel_rx) = oneshot::channel();

        let client = Client {
            responses: response_tx,
            action_requests_tx: action_tx.clone(),
            action_requests_rx: action_rx,
            stream,
            endpoint_readers: HashMap::new(),
            events_tx: user_events_tx,
            events_rx: Some(user_events_rx),
            close: cancel_tx,
        };

        tokio::spawn(async move { client.run().await });

        Ok(Self {
            tx: ClientHandleTx(action_tx),
            rx: ClientHandleRx {
                responses: response_rx,
            },
            _cancel_rx: cancel_rx,
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

    async fn observe_response(&mut self) -> Result<EndpointReader, Error> {
        match self.rx.next_response().await {
            Ok(ClientResponse::Observing(endpoint)) => Ok(endpoint),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    async fn event_response(&mut self) -> Result<EventReader, Error> {
        match self.rx.next_response().await {
            Ok(ClientResponse::Events(reader)) => Ok(reader),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    // TODO: We'd like to have something more, e.g.
    // observe(&mut self, thing: impl Into<DescribesEndpoint>).
    //
    // This way we can feed an EndpointWriter into it and use the id.

    /// Start observing the mock with the given name.
    pub async fn observe_tty(&mut self, path: &str) -> Result<EndpointReader, Error> {
        self.tx.observe_tty(path).await?;
        self.observe_response().await
    }

    /// Start observing the mock with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<EndpointReader, Error> {
        self.tx.observe_mock(name).await?;
        self.observe_response().await
    }

    async fn wait_for_control(&mut self) -> Result<Vec<EndpointWriter>, Error> {
        match self.rx.next_response().await {
            Ok(ClientResponse::Controlling(endpoints)) => {
                for endpoint in &endpoints {
                    info!(%endpoint, "Granted");
                }
                Ok(endpoints)
            }
            Ok(ClientResponse::Queued) => {
                info!("Queued");
                let after_queue = self.rx.next_response().await?;
                match after_queue {
                    ClientResponse::Controlling(endpoints) => {
                        for endpoint in &endpoints {
                            info!(%endpoint, "Granted");
                        }
                        Ok(endpoints)
                    }
                    _ => unreachable!(),
                }
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    async fn wait_for_one_writer(&mut self) -> Result<EndpointWriter, Error> {
        let mut writers = self.wait_for_control().await?;
        let writer = writers.remove(0);
        assert!(writers.is_empty());

        Ok(writer)
    }

    /// Start controlling the mock with the given name.
    pub async fn control_mock(&mut self, name: &str) -> Result<EndpointWriter, Error> {
        self.tx.control_mock(name).await?;
        self.wait_for_one_writer().await
    }

    /// Start controlling the mock with the given name.
    pub async fn control_tty(&mut self, path: &str) -> Result<EndpointWriter, Error> {
        self.tx.control_tty(path).await?;
        self.wait_for_one_writer().await
    }

    /// Start controlling any endpoint(s) with the matching label(s).
    pub async fn control_any<S: AsRef<str>>(
        &mut self,
        labels: &[S],
    ) -> Result<EndpointWriters, Error> {
        self.tx.control_any(labels).await?;
        self.wait_for_control().await.map(|ew| ew.into())
    }

    /// Start observing events from the server.
    pub async fn observe_events(&mut self) -> Result<EventReader, Error> {
        self.tx.observe_events().await?;
        self.event_response().await
    }

    /// Mutable borrow of the tx.
    pub fn tx_mut(&mut self) -> &mut ClientHandleTx {
        &mut self.tx
    }

    /// Mutable borrow of the rx.
    pub fn rx_mut(&mut self) -> &mut ClientHandleRx {
        &mut self.rx
    }
}
