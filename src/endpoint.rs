use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;

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

impl EndpointLabel {
    pub(crate) fn mock(name: &str) -> Self {
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
pub trait Endpoint {
    /// Get a receiver which receives messages which come from the wire.
    fn inbox(&self) -> broadcast::Receiver<SerialMessage>;

    /// Get a sender onto which we can put messages for writing to the wire.
    fn outbox(&self) -> mpsc::UnboundedSender<SerialMessage>;

    /// Some identifier of the endpoint.
    fn label(&self) -> EndpointLabel;
}
