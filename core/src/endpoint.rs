use std::sync::Arc;
use std::{borrow::Borrow, fmt::Display};
use std::{collections::HashSet, hash::Hash};

use futures::channel::mpsc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex, Semaphore};
use uuid::Uuid;

use crate::{
    mock::MockId,
    serial::{SerialMessage, SerialMessageBytes},
};

pub(crate) mod mock;
pub(crate) mod serial;

/// An endpoint a client may ask to observe.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone, PartialOrd, Ord)]
pub enum EndpointId {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(String),
}

impl From<InternalEndpointInfo> for EndpointId {
    fn from(iei: InternalEndpointInfo) -> Self {
        match iei.id {
            InternalEndpointId::Tty(tty) => Self::Tty(tty),
            InternalEndpointId::Mock(mock) => Self::Mock(mock.to_string()),
        }
    }
}

impl From<LabelledEndpointId> for EndpointId {
    fn from(lei: LabelledEndpointId) -> Self {
        lei.id
    }
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

    /// Associated [`Label`]s.
    pub labels: Labels,
}

impl LabelledEndpointId {
    /// An endpoint id with no labels.
    pub fn new(id: &EndpointId) -> Self {
        Self::new_with_labels::<&str>(id, &[])
    }

    /// An endpoint id with labels.
    pub fn new_with_labels<S: AsRef<str>>(id: &EndpointId, labels: &[S]) -> Self {
        Self {
            id: id.clone(),
            labels: labels.iter().collect(),
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
        if !&self.labels.is_empty() {
            write!(f, "{}, labels: {}", self.id, self.labels)
        } else {
            write!(f, "{}", self.id)
        }
    }
}

/// An endpoint as used internally.
/// May have extra internal fields not relevant to users,
/// which should look at [`EndpointId`] instead.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub enum InternalEndpointId {
    /// A tty/COM endpoint.
    Tty(String),

    /// An endpoint consisting of in-memory data,
    /// like lines of serial output.
    Mock(MockId),
}

/// An endpoint as seen internally.
#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
pub struct InternalEndpointInfo {
    /// The id of the endpoint.
    /// Unique per endpoint.
    pub id: InternalEndpointId,

    /// The labels associated with the endpoint.
    pub labels: Labels,
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
        if !&self.labels.is_empty() {
            write!(f, "{} [{}]", self.id, self.labels)
        } else {
            write!(f, "{}", self.id)
        }
    }
}

impl InternalEndpointInfo {
    pub(crate) fn new(id: InternalEndpointId, labels: Labels) -> Self {
        Self { id, labels }
    }
}

impl Display for InternalEndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalEndpointId::Tty(tty) => {
                // Reduce log verbosity
                let tty = if let Some(tty) = tty.strip_prefix("/dev/serial/by-id/") {
                    tty
                } else {
                    tty
                };

                write!(f, "{tty}")
            }
            InternalEndpointId::Mock(mock_id) => {
                write!(f, "{mock_id}")
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
    /// A new TTY endpoint identifier.
    pub fn tty(tty: &str) -> Self {
        Self::Tty(tty.into())
    }

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

impl AsRef<str> for Label {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for &Label {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Label {
    /// Create a new label.
    pub fn new<S: AsRef<str>>(label: S) -> Self {
        Self(label.as_ref().into())
    }
}

/// A collection of [`Label`]s.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct Labels(Vec<Label>);

impl Labels {
    /// Borrow the [`Label`]s as a [`HashSet`].
    pub fn as_hash_set(&self) -> HashSet<&Label> {
        HashSet::from_iter(self.0.iter())
    }

    /// The [`Label`]s as a [`HashSet`].
    pub fn into_hash_set(self) -> HashSet<Label> {
        HashSet::from_iter(self.0.into_iter())
    }

    /// Iterate over borrowed [`Label`]s.
    pub fn iter(&self) -> impl Iterator<Item = &Label> {
        self.0.iter()
    }

    /// Check if these labels are a superset of some other labels.
    pub fn is_superset(&self, other: &Labels) -> bool {
        self.as_hash_set().is_superset(&other.as_hash_set())
    }

    /// Check if the labels collection is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Push a new label.
    pub fn push<S>(&mut self, label: S)
    where
        S: AsRef<str>,
    {
        self.0.push(Label::new(label))
    }
}

impl AsRef<[Label]> for Labels {
    fn as_ref(&self) -> &[Label] {
        self.0.as_slice()
    }
}

impl IntoIterator for Labels {
    type Item = Label;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<S> FromIterator<S> for Labels
where
    S: AsRef<str>,
{
    fn from_iter<T: IntoIterator<Item = S>>(iter: T) -> Self {
        Labels(iter.into_iter().map(Label::new).collect())
    }
}

impl<S> From<S> for Labels
where
    S: AsRef<str>,
{
    fn from(label: S) -> Self {
        Self::from_iter([label])
    }
}

impl Display for Labels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for label in &self.0 {
            write!(f, "{label} ")?;
        }
        Ok(())
    }
}

/// An event on an endpoint.
#[derive(Debug, Clone)]
pub enum EndpointEvent {
    /// The endpoint was asked to put the given message on wire.
    ToWire(SerialMessageBytes),

    /// The endpoint received the following message from wire.
    FromWire(SerialMessageBytes),
}

impl Display for EndpointEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            EndpointEvent::ToWire(b) => {
                write!(f, "ToWire: {}", SerialMessage::new_lossy(b))
            }
            EndpointEvent::FromWire(b) => {
                write!(f, "FromWire: {}", SerialMessage::new_lossy(b))
            }
        }
    }
}

impl EndpointEvent {
    /// Unwrap the event as something that was sent to wire, else panic.
    pub fn into_to_wire(self) -> SerialMessageBytes {
        if let Self::ToWire(v) = self {
            v
        } else {
            panic!("Was not `ToWire`: {self:?}")
        }
    }

    /// Unwrap the event as something that was received from wire, else panic.
    pub fn into_from_wire(self) -> SerialMessageBytes {
        if let Self::FromWire(v) = self {
            v
        } else {
            panic!("Was not `FromWire`: {self:?}")
        }
    }

    /// Attempt to unwrap the event as something that was sent to wire.
    pub fn as_to_wire(&self) -> Option<&SerialMessageBytes> {
        if let Self::ToWire(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Attempt to unwrap the event as something that was received from wire.
    pub fn as_from_wire(&self) -> Option<&SerialMessageBytes> {
        if let Self::FromWire(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

/// An endpoint is something which can accept serial messages for writing,
/// and generates serial messages for reading.
pub(crate) trait Endpoint {
    /// Get a receiver which receives messages which come from the wire.
    fn events(&self) -> broadcast::Receiver<EndpointEvent>;

    /// Get the semaphore needed to be able to user the endpoint as a writer.
    fn semaphore(&self) -> EndpointSemaphore;

    /// The sender which should be only used with a permit.
    /// TODO: Hide?
    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessageBytes>;

    /// An internal identifier of the endpoint.
    fn internal_endpoint_id(&self) -> InternalEndpointId;

    /// The labels associated with this endpoint.
    fn labels(&self) -> Labels {
        Labels::default()
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
