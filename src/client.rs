use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll},
};

// use color_eyre::Report;
use futures::{
    channel::{mpsc, oneshot},
    Sink, SinkExt, StreamExt,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, info_span, Instrument};

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

struct Endpoint {
    messages: mpsc::UnboundedReceiver<SerialMessageBytes>,
    user_wants_message: mpsc::UnboundedReceiver<oneshot::Sender<SerialMessageBytes>>,
}

impl Endpoint {
    async fn run(mut self) {
        loop {
            let user_tx = self.user_wants_message.next().await.expect("TODO");
            let message = self.messages.next().await.expect("TODO");

            user_tx.send(message).expect("TODO");
        }
    }
}

#[derive(Clone)]
struct EndpointHandle {
    _id: LabelledEndpointId,

    messages: mpsc::UnboundedSender<SerialMessageBytes>,
    user_wants_message: mpsc::UnboundedSender<oneshot::Sender<SerialMessageBytes>>,
}

impl EndpointHandle {
    fn new(id: LabelledEndpointId) -> Self {
        let (messages_tx, messages_rx) = mpsc::unbounded();
        let (user_wants_message_tx, user_wants_message_rx) = mpsc::unbounded();

        let endpoint = Endpoint {
            messages: messages_rx,
            user_wants_message: user_wants_message_rx,
        };
        tokio::spawn(async move { endpoint.run().await });

        Self {
            _id: id,
            messages: messages_tx,
            user_wants_message: user_wants_message_tx,
        }
    }

    async fn next_message(&mut self) -> SerialMessageBytes {
        let (tx, rx) = oneshot::channel();
        self.user_wants_message.send(tx).await.expect("TODO");

        debug!("Awaiting a message");
        rx.await.unwrap()
    }
}

struct EndpointReader {
    _id: LabelledEndpointId,
    pub messages: mpsc::UnboundedReceiver<SerialMessageBytes>,
}

impl EndpointReader {
    // fn new(id: LabelledEndpointId) -> Self {
    //     let (messages_tx, messages_rx) = mpsc::unbounded();
    //     let (user_wants_message_tx, user_wants_message_rx) = mpsc::unbounded();

    //     let endpoint = Endpoint {
    //         messages: messages_rx,
    //         user_wants_message: user_wants_message_rx,
    //     };
    //     tokio::spawn(async move { endpoint.run().await });

    //     Self {
    //         _id: id,
    //         messages: messages_tx,
    //         user_wants_message: user_wants_message_tx,
    //     }
    // }

    async fn next_message(&mut self) -> SerialMessageBytes {
        self.messages.next().await.unwrap()
    }
}

struct Client {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,

    /// Writes any non-async responses
    responses: mpsc::UnboundedSender<ResponseResult>,

    action_requests: mpsc::UnboundedReceiver<Action>,

    message_poll: mpsc::UnboundedReceiver<(
        LabelledEndpointId,
        oneshot::Sender<Result<SerialMessageBytes, Error>>,
    )>,
    endpoint_handles: HashMap<LabelledEndpointId, EndpointHandle>,
}

impl Client {
    async fn handle_websocket_message(
        message: Option<Result<tungstenite::protocol::Message, tungstenite::Error>>,
        endpoint_handles: &mut HashMap<LabelledEndpointId, EndpointHandle>,
        responses: &mut mpsc::UnboundedSender<ResponseResult>,
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
            Err(_) => {
                if let Err(send_error) = responses.send(response).await {
                    error!(?send_error, "Could not send message to client");
                }
                return;
            }
        };

        use actions::Sync::*;
        let response = match response {
            Response::Sync(response) => match response {
                WriteOk | ControlQueue(_) => response,
                Observing(ref ids) => {
                    for id in ids {
                        endpoint_handles
                            .entry(id.clone())
                            .or_insert_with(|| EndpointHandle::new(id.clone()));
                    }
                    response
                }
                ControlGranted(ref ids) => {
                    for id in ids {
                        endpoint_handles
                            .entry(id.clone())
                            .or_insert_with(|| EndpointHandle::new(id.clone()));
                    }
                    response
                }
            },
            Response::Async(Async::Message { endpoint, message }) => {
                let eh = endpoint_handles
                    .entry(endpoint.clone())
                    .or_insert_with(|| EndpointHandle::new(endpoint));

                eh.messages
                    .unbounded_send(message)
                    .expect("Should be alive");
                return;
            }
        };

        if let Err(e) = responses.send(Ok(Response::Sync(response))).await {
            error!(?e, "Could not send message to client");
        }
    }

    async fn handle_user_message_poll(
        poll: Option<(
            LabelledEndpointId,
            oneshot::Sender<Result<SerialMessageBytes, Error>>,
        )>,
        endpoint_handles: &mut HashMap<LabelledEndpointId, EndpointHandle>,
    ) {
        let (id, tx) = poll.expect("TODO");

        let mut handle = match endpoint_handles.get(&id) {
            Some(handle) => handle.clone(),
            None => {
                debug!(?id, "Asked for endpoint we don't have");
                tx.send(Err(Error::NoSuchEndpoint(id.to_string())))
                    .expect("TODO");
                return;
            }
        };

        tokio::spawn(async move {
            let id = handle._id.clone();
            let message = handle
                .next_message()
                .instrument(info_span!("Handle rx", %id))
                .await;

            tx.send(Ok(message)).expect("TODO");
        });
    }

    async fn run(self) {
        let (mut ws_tx, mut ws_rx) = self.stream.split();
        let mut msg_poll = self.message_poll;

        let mut actions_rx = self.action_requests;
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

        let mut endpoint_handles = self.endpoint_handles;

        let response_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    ws_msg = ws_rx.next() => {
                        debug!("New websocket message");
                        Self::handle_websocket_message(ws_msg, &mut endpoint_handles, &mut response_tx).await;
                    }
                    msg_poll = msg_poll.next() => {
                        debug!("New user message request");
                        Self::handle_user_message_poll(msg_poll, &mut endpoint_handles).await;
                    }
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
    responses: mpsc::UnboundedReceiver<ResponseResult>,
    messages: mpsc::UnboundedSender<(
        LabelledEndpointId,
        oneshot::Sender<Result<SerialMessageBytes, Error>>,
    )>,
}

impl ClientHandleRx {
    /// Await the next response from the transport.
    pub async fn next_response(&mut self) -> ResponseResult {
        self.responses
            .next()
            .await
            .ok_or_else(|| Error::WebsocketIssue("Next was None".into()))?
    }

    /// Await the next message from the given endpoint.
    pub async fn next_message(
        &mut self,
        endpoint: &LabelledEndpointId,
    ) -> Result<SerialMessageBytes, Error> {
        let (tx, rx) = oneshot::channel();

        self.messages
            .send((endpoint.clone(), tx))
            .await
            .expect("TODO, client should be alive?");

        debug!("Awaiting message");
        rx.await.expect("TODO, client should be alive here too?")
    }
}

impl ClientHandle {
    /// Given a port and an address on the format `{}`
    pub async fn new(address: &str, port: u16) -> Result<Self, Error> {
        let (stream, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}:{port}/ws")).await?;

        let (action_tx, action_rx) = mpsc::unbounded();
        let (response_tx, response_rx) = mpsc::unbounded();

        let (message_poll_tx, message_poll_rx) = mpsc::unbounded();

        let client = Client {
            responses: response_tx,
            action_requests: action_rx,
            stream,
            message_poll: message_poll_rx,
            endpoint_handles: HashMap::new(),
        };

        tokio::spawn(async move { client.run().await });

        Ok(Self {
            tx: ClientHandleTx(action_tx),
            rx: ClientHandleRx {
                responses: response_rx,
                messages: message_poll_tx,
            },
        })
    }

    /// Split into sender and receiver parts of the client handle.
    pub fn split(self) -> (ClientHandleTx, ClientHandleRx) {
        (self.tx, self.rx)
    }

    async fn observe_response(&mut self) -> Result<Vec<LabelledEndpointId>, Error> {
        match self.rx.next_response().await {
            Ok(Response::Sync(actions::Sync::Observing(ids))) => Ok(ids),
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    /// Start observing the mock with the given name.
    pub async fn observe_tty(&mut self, path: &str) -> Result<Vec<LabelledEndpointId>, Error> {
        self.tx.observe_tty(path).await?;
        self.observe_response().await
    }

    /// Start observing the mock with the given name.
    pub async fn observe_mock(&mut self, name: &str) -> Result<Vec<LabelledEndpointId>, Error> {
        self.tx.observe_mock(name).await?;
        self.observe_response().await
    }

    async fn wait_for_control(&mut self) -> Result<Vec<LabelledEndpointId>, Error> {
        match self.rx.next_response().await {
            Ok(Response::Sync(actions::Sync::ControlGranted(control_granted))) => {
                info!(?control_granted, "Granted");
                Ok(control_granted)
            }
            Ok(Response::Sync(actions::Sync::ControlQueue(queue))) => {
                info!(?queue, "Queued");
                let after_queue = self.rx.next_response().await?;
                match after_queue {
                    Response::Sync(actions::Sync::ControlGranted(control_granted)) => {
                        Ok(control_granted)
                    }
                    _ => unreachable!(),
                }
            }
            Ok(_) => unreachable!(),
            Err(e) => Err(e),
        }
    }

    /// Start controlling the mock with the given name.
    pub async fn control_mock(&mut self, name: &str) -> Result<Vec<LabelledEndpointId>, Error> {
        self.tx.control_mock(name).await?;
        self.wait_for_control().await
    }

    /// Start controlling the mock with the given name.
    pub async fn control_tty(&mut self, path: &str) -> Result<Vec<LabelledEndpointId>, Error> {
        self.tx.control_tty(path).await?;
        self.wait_for_control().await
    }

    /// TODO
    pub async fn next_message(
        &mut self,
        endpoint: &LabelledEndpointId,
    ) -> Result<SerialMessage, Error> {
        Ok(String::from_utf8_lossy(
            &self
                .rx
                .next_message(endpoint)
                .instrument(info_span!("Next Message", %endpoint))
                .await?,
        )
        .to_string())
    }

    /// Write TODO
    pub async fn write<M>(&mut self, endpoint: &LabelledEndpointId, message: M) -> Result<(), Error>
    where
        M: AsRef<[u8]>,
    {
        self.tx
            .send(Action::write_bytes(&endpoint.id, message.as_ref().into()))
            .await
            .expect("TODO");

        match self.rx.next_response().await? {
            Response::Sync(actions::Sync::WriteOk) => Ok(()),
            _ => unreachable!(),
        }
    }
}
