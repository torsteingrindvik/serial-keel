use std::hash::Hash;
use std::sync::Arc;
use std::{borrow::Borrow, fmt::Display};

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex, Semaphore};
use uuid::Uuid;

use crate::{mock::MockId, serial::serial_port::SerialMessage};

pub(crate) mod mock;
pub(crate) mod serial;

/// An endpoint a client may ask to observe.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum EndpointId {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(String),
}

impl Display for EndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointId::Tty(tty) => write!(f, "tty: {tty}"),
            EndpointId::Mock(mock) => write!(f, "mock: {mock}"),
        }
    }
}

/// An enpoint and the labels associated with it, if any.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct LabelledEndpointId {
    /// The [`EndpointId`].
    pub id: EndpointId,

    /// Associated [`Label`]s, if any.
    pub labels: Option<Vec<Label>>,
}

impl LabelledEndpointId {
    /// An endpoint id with no labels.
    pub fn new(id: &EndpointId) -> Self {
        Self {
            id: id.clone(),
            labels: None,
        }
    }
}

impl From<InternalEndpointInfo> for LabelledEndpointId {
    fn from(iei: InternalEndpointInfo) -> Self {
        Self {
            id: iei.id.into(),
            labels: iei.labels,
        }
    }
}

impl Display for LabelledEndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(labels) = &self.labels {
            write!(f, "{}, labels:", self.id)?;
            for label in labels {
                write!(f, " {label}")?;
            }
            Ok(())
        } else {
            write!(f, "{}", self.id)
        }
    }
}

/// An endpoint as used internally.
/// May have extra internal fields not relevant to users,
/// which should look at [`Endpointid`] instead.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) enum InternalEndpointId {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(MockId),
}

/// TODO
#[derive(Debug, Clone, Eq)]
pub(crate) struct InternalEndpointInfo {
    pub(crate) id: InternalEndpointId,
    pub(crate) labels: Option<Vec<Label>>,
}

impl PartialEq for InternalEndpointInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Borrow<InternalEndpointId> for InternalEndpointInfo {
    fn borrow(&self) -> &InternalEndpointId {
        &self.id
    }
}

impl Hash for InternalEndpointInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Display for InternalEndpointInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(labels) = &self.labels {
            write!(f, "{}, labels:", self.id)?;
            for label in labels {
                write!(f, " {label}")?;
            }
            Ok(())
        } else {
            write!(f, "{}", self.id)
        }
    }
}

impl InternalEndpointInfo {
    pub(crate) fn new(id: InternalEndpointId, labels: Option<Vec<Label>>) -> Self {
        Self { id, labels }
    }
}

impl Display for InternalEndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalEndpointId::Tty(tty) => write!(f, "{tty}"),
            InternalEndpointId::Mock(mock_id) => {
                write!(f, "{}", mock_id)
            }
        }
    }
}

impl From<InternalEndpointId> for EndpointId {
    fn from(internal: InternalEndpointId) -> Self {
        match internal {
            InternalEndpointId::Tty(tty) => Self::Tty(tty),
            InternalEndpointId::Mock(mock_id) => Self::Mock(mock_id.name),
        }
    }
}

impl EndpointId {
    /// A id for a mock endpoint.
    pub fn mock(name: &str) -> Self {
        Self::Mock(name.into())
    }

    /// Borrow endpoint id as the mock variant.
    pub fn as_mock(&self) -> Option<&String> {
        if let Self::Mock(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Borrow endpoint id as the TTY variant.
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    pub(crate) inner: Arc<Semaphore>,
    pub(crate) id: EndpointSemaphoreId,
}

impl Default for EndpointSemaphore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Semaphore::new(1)),
            id: EndpointSemaphoreId(Uuid::new_v4()),
        }
    }
}

/// A label an endpoint may be associated with.
///
/// May be used to have a one-to-many mapping to endpoints.
///
/// Using the same label for several endpoints
/// allows querying the label and thus queuing on
/// endpoints sharing the label.
/// This allows control access over the first
/// endpoint available with the matching label.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct Label(pub String);

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Label {
    /// Create a new label.
    pub fn new<S: AsRef<str>>(label: S) -> Self {
        Self(label.as_ref().into())
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

    /// An internal identifier of the endpoint.
    fn internal_endpoint_id(&self) -> InternalEndpointId;

    /// An alias for the endpoint.
    /// If given, users may ask for
    fn labels(&self) -> Option<Vec<Label>> {
        None
    }
}

pub(crate) trait EndpointExt: Endpoint {
    fn semaphore_id(&self) -> EndpointSemaphoreId {
        self.semaphore().id
    }
}

/// Automatically provide [`EndpointExt`] for things implementing
/// [`Endpoint`].
/// The `?Sized` allows that to work if `T` is inside a `Box` too.
impl<T> EndpointExt for T where T: Endpoint + ?Sized {}
