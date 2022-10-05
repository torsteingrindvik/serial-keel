#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;

pub use nordic_types::serial::SerialMessage;

use futures::{channel::mpsc, Sink, Stream};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};

/// Represents a tty path on unix,
/// or a COM string on Windows.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Tty {
    #[cfg(windows)]
    path: String,

    #[cfg(unix)]
    path: PathBuf,
}

/// An endpoint a client may ask to observe.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EndpointLabel {
    // TODO: nordic-types
    /// A tty/COM endpoint.
    Tty(Tty),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(String),
}

/// A handle to an endpoint.
/// Can be shared around.
/// Allows interacting with the endpoint.
#[derive(Debug, Clone)]
pub struct EndpointHandle {
    arriving_messages: broadcast::Sender<SerialMessage>,
    messages_to_send: Arc<Mutex<mpsc::UnboundedSender<SerialMessage>>>,
}

impl EndpointHandle {
    pub(crate) fn subscriber(&self) -> broadcast::Receiver<SerialMessage> {
        self.arriving_messages.subscribe()
    }
}

/// An endpoint is something which can accept serial messages for writing,
/// and generates serial messages for reading.
pub trait Endpoint: Sink<SerialMessage> + Stream<Item = SerialMessage> {
    fn handle(&self) -> EndpointHandle;
}

mod mock;
