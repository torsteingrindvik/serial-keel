use std::fmt::Display;
use std::sync::Arc;

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, Mutex, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tracing::warn;

use crate::{mock::MockId, serial::serial_port::SerialMessage};

pub(crate) mod mock;
pub(crate) mod serial;

// Represents a tty path on unix,
// or a COM string on Windows.
// #[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
// pub struct Tty {
//     #[cfg(windows)]
//     path: String,

//     #[cfg(unix)]
//     path: PathBuf,
// }

// impl Display for Tty {
//     #[cfg(windows)]
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self.path)
//     }

//     #[cfg(unix)]
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self.path.to_string_lossy())
//     }
// }

// impl Tty {
//     /// Create a tty.
//     #[cfg(windows)]
//     pub fn new<P: AsRef<Path>>(path: P) -> Self {
//         #[cfg(windows)]
//         Self {
//             path: path.as_ref().to_string_lossy().into_owned(),
//         }
//     }

//     /// Create a tty.
//     #[cfg(not(windows))]
//     pub fn new<P: AsRef<Path>>(path: P) -> Self {
//         Self {
//             path: path.as_ref().into(),
//         }
//     }
// }

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
pub struct OutboxQueue(pub(crate) oneshot::Receiver<Outbox>);

/// Exclusive access to writing to an endpoint is granted via this.
/// When dropped, the permit is freed and someone else may be granted access.
#[derive(Debug)]
pub struct Outbox {
    _permit: OwnedSemaphorePermit,
    pub(crate) inner: mpsc::UnboundedSender<SerialMessage>,
}

/// When requesting exclusive access,
/// it might be granted.
/// If noone is using the outbox, it's granted right away.
/// Else a queue is provided which can be awaited
#[derive(Debug)]
pub enum MaybeOutbox {
    /// The outbox was available.
    Available(Outbox),

    /// The outbox was taken.
    /// [`OutboxQueue`] can be awaited to gain access.
    Busy(OutboxQueue),
}

/// An endpoint is something which can accept serial messages for writing,
/// and generates serial messages for reading.
pub(crate) trait Endpoint {
    /// Get a receiver which receives messages which come from the wire.
    fn inbox(&self) -> broadcast::Receiver<SerialMessage>;

    /// Get the semaphore needed to be able to user the endpoint as a writer.
    fn semaphore(&self) -> Arc<Semaphore>;

    /// The sender which should be only used with a permit.
    /// TODO: Hide?
    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessage>;

    /// Get an outbox for sending messages, if available.
    /// If not it must be awaited.
    fn outbox(&self) -> MaybeOutbox {
        match self.semaphore().clone().try_acquire_owned() {
            Ok(permit) => MaybeOutbox::Available(Outbox {
                _permit: permit,
                inner: self.message_sender().clone(),
            }),
            Err(TryAcquireError::NoPermits) => {
                let (permit_tx, permit_rx) = oneshot::channel();
                let permit_fut = self.semaphore().clone().acquire_owned();
                let outbox = self.message_sender().clone();

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

    /// Some identifier of the endpoint.
    fn label(&self) -> InternalEndpointLabel;
}
