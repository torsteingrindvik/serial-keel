#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use std::{fmt::Display, path::Path};

pub use nordic_types::serial::SerialMessage;

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, Mutex, OwnedSemaphorePermit};

pub(crate) mod mock;

/// Represents a tty path on unix,
/// or a COM string on Windows.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct Tty {
    #[cfg(windows)]
    path: String,

    #[cfg(unix)]
    path: PathBuf,
}

impl Display for Tty {
    #[cfg(windows)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path)
    }

    #[cfg(unix)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.to_string_lossy())
    }
}

impl Tty {
    /// Create a tty.
    #[cfg(windows)]
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        #[cfg(windows)]
        Self {
            path: path.as_ref().to_string_lossy().into_owned(),
        }
    }

    /// Create a tty.
    #[cfg(not(windows))]
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().into(),
        }
    }
}

/// An endpoint a client may ask to observe.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum EndpointLabel {
    // TODO: nordic-types
    /// A tty/COM endpoint.
    Tty(Tty),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(String),
}

/// An endpoint as used internally.
/// May have extra internal fields not relevant to users,
/// which should look at [`EndpointLabel`] instead.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) enum InternalEndpointLabel {
    // TODO: nordic-types
    /// A tty/COM endpoint.
    Tty(Tty),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(mock::MockId),
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

// TODO: Testcases!
//
// Especially:
//
// 1. Available, hold it
// 2. Busy, get queued
// 3. Busy, also queed
// 4. First in queue drops
// 5. Semaphore dropped
// 6. Check that first in queue is ignored, second in place gets it
//
// Also check things like if there was a queue, but all queuers dropped, _then_ someone arrives.
// And so on.

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

    /// Get an outbox for sending messages, if available.
    /// If not it must be awaited.
    fn outbox(&self) -> MaybeOutbox;

    /// Some identifier of the endpoint.
    fn label(&self) -> InternalEndpointLabel;
}
