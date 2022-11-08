use std::fmt::Display;
use std::sync::Arc;

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, Mutex, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tracing::warn;
use uuid::Uuid;

use crate::{mock::MockId, serial::serial_port::SerialMessage};

pub(crate) mod mock;
pub(crate) mod serial;

/// An endpoint a client may ask to observe.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum EndpointLabel {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(String),
}

impl Display for EndpointLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointLabel::Tty(tty) => write!(f, "tty: {tty}"),
            EndpointLabel::Mock(mock) => write!(f, "mock: {mock}"),
        }
    }
}

/// An endpoint as used internally.
/// May have extra internal fields not relevant to users,
/// which should look at [`EndpointLabel`] instead.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) enum InternalEndpointLabel {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(MockId),
}

impl Display for InternalEndpointLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalEndpointLabel::Tty(tty) => write!(f, "{tty}"),
            InternalEndpointLabel::Mock(mock_id) => {
                write!(f, "{}", mock_id)
            }
        }
    }
}

impl From<InternalEndpointLabel> for EndpointLabel {
    fn from(internal: InternalEndpointLabel) -> Self {
        match internal {
            InternalEndpointLabel::Tty(tty) => Self::Tty(tty),
            InternalEndpointLabel::Mock(mock_id) => Self::Mock(mock_id.name),
        }
    }
}

impl EndpointLabel {
    /// A label for a mock endpoint.
    pub fn mock(name: &str) -> Self {
        Self::Mock(name.into())
    }

    /// Borrow endpoint label as the mock variant.
    pub fn as_mock(&self) -> Option<&String> {
        if let Self::Mock(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrow endpoint label as the TTY variant.
    pub fn as_tty(&self) -> Option<&String> {
        if let Self::Tty(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

/// A handle to an endpoint.
#[derive(Debug)]
pub struct EndpointHandle {
    /// Messages the endpoint reads will be forwarded here.
    /// Therefore this can be used to listen to incoming messages.
    pub arriving_messages: broadcast::Receiver<SerialMessage>,

    /// The endpoint should write these messages onto wire.
    pub messages_to_send: Arc<Mutex<mpsc::UnboundedSender<SerialMessage>>>,
}

/// If a user requests exclusive control over writing to an endpoint but
/// someone else has it, they may wait for access here.
#[derive(Debug)]
pub(crate) struct OutboxQueue(pub(crate) oneshot::Receiver<Outbox>);

// Maybe something like this?
// It would be even nicer if the regular Outbox was more flexible.
//
// We could expose a function on the Control Center which takes an EndpointSemaphoreId,
// and returns the associated outboxes!
// Nice!
#[derive(Debug)]
pub(crate) struct Outboxes {
    _permit: OwnedSemaphorePermit,
}

/// Exclusive access to writing to an endpoint is granted via this.
/// When dropped, the permit is freed and someone else may be granted access.
#[derive(Debug)]
pub(crate) struct Outbox {
    _permit: OwnedSemaphorePermit,

    pub(crate) inner: mpsc::UnboundedSender<SerialMessage>,
}

/// When requesting exclusive access,
/// it might be granted.
/// If noone is using the outbox, it's granted right away.
/// Else a queue is provided which can be awaited
#[derive(Debug)]
pub(crate) enum MaybeOutbox {
    /// The outbox was available.
    Available(Outbox),

    /// The outbox was taken.
    /// [`OutboxQueue`] can be awaited to gain access.
    Busy(OutboxQueue),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EndpointSemaphoreId(Uuid);
impl Display for EndpointSemaphoreId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Endpoints which should be grouped in terms of being controlled
/// (so controlling one means controlling all) should clone this
/// endpoint semaphore.
///
/// This way when an actual permit is obtained, we can map that to
/// other permits (or something? Todo)
#[derive(Debug, Clone)]
pub(crate) struct EndpointSemaphore {
    pub(crate) semaphore: Arc<Semaphore>,
    pub(crate) id: EndpointSemaphoreId,
}

impl Default for EndpointSemaphore {
    fn default() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            id: EndpointSemaphoreId(Uuid::new_v4()),
        }
    }
}

/// An endpoint is something which can accept serial messages for writing,
/// and generates serial messages for reading.
pub(crate) trait Endpoint {
    /// Get a receiver which receives messages which come from the wire.
    fn inbox(&self) -> broadcast::Receiver<SerialMessage>;

    /// Get the semaphore needed to be able to user the endpoint as a writer.
    fn semaphore(&self) -> EndpointSemaphore;

    /// The sender which should be only used with a permit.
    /// TODO: Hide?
    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessage>;

    /// Some identifier of the endpoint.
    fn label(&self) -> InternalEndpointLabel;
}

pub(crate) trait EndpointExt: Endpoint {
    /// Get an outbox for sending messages, if available.
    /// If not it must be awaited.
    fn outbox(&self) -> MaybeOutbox {
        match self.semaphore().semaphore.try_acquire_owned() {
            Ok(permit) => MaybeOutbox::Available(Outbox {
                _permit: permit,
                inner: self.message_sender(),
            }),
            Err(TryAcquireError::NoPermits) => {
                let (permit_tx, permit_rx) = oneshot::channel();
                let permit_fut = self.semaphore().semaphore.acquire_owned();
                let outbox = self.message_sender();

                tokio::spawn(async move {
                    if let Ok(permit) = permit_fut.await {
                        if permit_tx
                            .send(Outbox {
                                _permit: permit,
                                inner: outbox,
                            })
                            .is_err()
                        {
                            warn!("Permit acquired but no user to receive it")
                        };
                    } else {
                        warn!("Could not get permit- endpoint closed?")
                    }
                });

                MaybeOutbox::Busy(OutboxQueue(permit_rx))
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        }
    }

    fn semaphore_id(&self) -> EndpointSemaphoreId {
        self.semaphore().id
    }
}

/// Automatically provide [`EndpointExt`] for things implementing
/// [`Endpoint`].
/// The `?Sized` allows that to work if `T` is inside a `Box` too.
impl<T> EndpointExt for T where T: Endpoint + ?Sized {}
