#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;
use std::{fmt::Display, path::Path};

pub use nordic_types::serial::SerialMessage;

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};

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
            InternalEndpointLabel::Tty(tty) => write!(f, "{}", tty.path.as_str()),
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

/// An endpoint is something which can accept serial messages for writing,
/// and generates serial messages for reading.
pub(crate) trait Endpoint {
    /// Get a receiver which receives messages which come from the wire.
    fn inbox(&self) -> broadcast::Receiver<SerialMessage>;

    /// Get a sender onto which we can put messages for writing to the wire.
    fn outbox(&self) -> mpsc::UnboundedSender<SerialMessage>;

    /// Some identifier of the endpoint.
    fn label(&self) -> InternalEndpointLabel;
}
