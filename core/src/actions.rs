use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    endpoint::{EndpointId, Label, LabelledEndpointId, Labels},
    error, events,
    serial::{SerialMessage, SerialMessageBytes},
};

/// Actions user can ask of the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum Action {
    /// Start controlling the given endpoint.
    ///
    /// This allows reading output from the given endpoint,
    /// but also exclusively writing to the endpoint.
    ///
    /// There may be many concurrent observers,
    /// but only a single controller.
    Control(EndpointId),

    /// Start controlling any endpoint matching the given labels.
    ControlAny(Labels),

    /// Start observing the given endpoint.
    ///
    /// The user may only read output from the given endpoint.
    ///
    /// There may be many concurrent observers,
    /// but only a single controller.
    Observe(EndpointId),

    /// Put this message on the wire for the given endpoint.
    Write((EndpointId, SerialMessage)),

    /// Put these bytes on the wire for the given endpoint.
    WriteBytes((EndpointId, SerialMessageBytes)),

    /// Start receiving events.
    ///
    /// This will send all events to the client, including:
    /// - Users connecting and disconnecting
    /// - Endpoint messages
    /// - Endpoint queue updates
    /// and more.
    ObserveEvents,
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Control(e) => write!(f, "control: {e}"),
            Action::Observe(e) => write!(f, "observe: {e}"),
            Action::Write((e, msg)) => {
                write!(f, "write: {e}, msg: {msg}")
            }
            Action::ControlAny(labels) => {
                write!(f, "control any: {labels}")
            }
            Action::WriteBytes((e, bytes)) => {
                write!(
                    f,
                    "write: {e}, msg: [{:?}]..",
                    &bytes[0..bytes.len().min(16)]
                )
            }
            Action::ObserveEvents => write!(f, "observe events"),
        }
    }
}

impl Action {
    /// Create a control action.
    pub fn control(id: &EndpointId) -> Self {
        Self::Control(id.clone())
    }

    /// Create a control mock action.
    pub fn control_mock(name: &str) -> Self {
        Self::control(&EndpointId::mock(name))
    }

    /// An example of requesting control of a mock.
    pub fn example_control_mock() -> Self {
        Self::control_mock("some-mock")
    }

    /// Create a control tty action.
    pub fn control_tty(path: &str) -> Self {
        Self::control(&EndpointId::tty(path))
    }

    /// An example of requesting control of a TTY.
    pub fn example_control_tty() -> Self {
        Self::control_mock("/dev/ttyACM123")
    }

    /// Create a control any action.
    pub fn control_any<S: AsRef<str>>(labels: &[S]) -> Self {
        Self::ControlAny(labels.iter().map(Label::new).collect())
    }

    /// An example of requesting control of any matching endpoint.
    pub fn example_control_any() -> Self {
        Self::control_any(&["my-label", "blue-device"])
    }

    /// Create an observe action.
    pub fn observe(id: &EndpointId) -> Self {
        Self::Observe(id.clone())
    }

    /// Create an observe TTY action.
    pub fn observe_tty(path: &str) -> Self {
        Self::Observe(EndpointId::tty(path))
    }

    /// An example of requesting to observe a TTY.
    pub fn example_observe_tty() -> Self {
        Self::observe_tty("/dev/ttyACM123")
    }

    /// Create an observe mock action.
    pub fn observe_mock(name: &str) -> Self {
        Self::Observe(EndpointId::mock(name))
    }

    /// An example of requesting to observe a mock.
    pub fn example_observe_mock() -> Self {
        Self::observe_mock("some-mock")
    }

    /// Create a write action.
    pub fn write(id: &EndpointId, message: SerialMessage) -> Self {
        Self::Write((id.clone(), message))
    }

    /// An example of a write message to a TTY endpoint.
    pub fn example_write() -> Self {
        Self::Write((EndpointId::tty("/dev/ttyACMx"), "This is a message".into()))
    }

    /// Create a write bytes action.
    pub fn write_bytes(id: &EndpointId, bytes: SerialMessageBytes) -> Self {
        Self::WriteBytes((id.clone(), bytes))
    }

    /// An example of a writing bytes to a mock endpoint.
    pub fn example_write_bytes() -> Self {
        Self::WriteBytes((
            EndpointId::mock("/mock/ttyACMx"),
            b"This is a message".to_vec(),
        ))
    }

    /// Create an observe events action.
    pub fn observe_events() -> Self {
        Self::ObserveEvents
    }

    /// An example of an observe events action.
    pub fn example_observe_events() -> Self {
        Self::observe_events()
    }

    /// Turn an action into serialized json.
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).expect("Should serialize well")
    }
}

/// A response type of "sync nature"- a direct response to a request.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Sync {
    /// The write action was successful.
    WriteOk,

    /// Now observing the following endpoint.
    Observing(LabelledEndpointId),

    /// Now receiving events.
    ObservingEvents,

    /// The requested endpoint was busy.
    /// When available, access is granted and
    /// [`Response::ControlGranted(_)`] is sent.
    ControlQueue(Vec<LabelledEndpointId>),

    /// The requested endpoint is now exclusively in use by the user.
    /// Writing to this endpoint is now possible.
    ControlGranted(Vec<LabelledEndpointId>),
}

/// An async response type- might originate on the server side at any time.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Async {
    /// An endpoint sent a message.
    Message {
        /// Which endpoint sent a message.
        endpoint: LabelledEndpointId,

        /// The message contents.
        message: SerialMessageBytes,
    },

    /// An event.
    Event(events::TimestampedEvent),
}

/// Responses the server will send to connected users.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Response {
    /// A synchronous response in the sense that it's sent directly after a user
    /// request.
    Sync(Sync),

    /// An async message- the server might send this at any time and not in response to any
    /// particular request.
    Async(Async),
}

impl Response {
    pub(crate) fn write_ok() -> Self {
        Self::Sync(Sync::WriteOk)
    }

    /// An example of a message write OK response.
    pub fn example_write_ok() -> Self {
        Self::write_ok()
    }

    pub(crate) fn observing_events() -> Self {
        Self::Sync(Sync::ObservingEvents)
    }

    /// An example of an observe events OK response.
    pub fn example_observing_events() -> Self {
        Self::observing_events()
    }

    pub(crate) fn message(endpoint: LabelledEndpointId, message: SerialMessageBytes) -> Self {
        Self::Async(Async::Message { endpoint, message })
    }

    /// An example of a new message response. These are async and might appear at any time after a user has
    /// started observing the related endpoint.
    pub fn example_new_message() -> Self {
        Self::message(
            LabelledEndpointId::new(&EndpointId::tty("COM0")),
            "Hello World!".into(),
        )
    }

    pub(crate) fn observing(id: LabelledEndpointId) -> Self {
        Self::Sync(Sync::Observing(id))
    }

    /// An example of a new message response.
    pub fn example_observing() -> Self {
        Self::observing(LabelledEndpointId::new_with_labels(
            &EndpointId::tty("/dev/ttyACM1"),
            &["secure"],
        ))
    }

    pub(crate) fn control_granted(granted: Vec<LabelledEndpointId>) -> Self {
        Self::Sync(Sync::ControlGranted(granted))
    }

    /// An example of a control granted response.
    pub fn example_control_granted() -> Self {
        Self::control_granted(vec![
            LabelledEndpointId::new(&EndpointId::tty("COM0")),
            LabelledEndpointId::new_with_labels(
                &EndpointId::tty("/dev/ttyACMx"),
                &["device-type-1", "server-room-foo"],
            ),
        ])
    }

    pub(crate) fn control_queue(queued_on: Vec<LabelledEndpointId>) -> Self {
        Self::Sync(Sync::ControlQueue(queued_on))
    }

    /// An example response when the user gets queued instead of being granted access.
    pub fn example_control_queue() -> Self {
        Self::control_queue(vec![
            LabelledEndpointId::new(&EndpointId::tty("COM0")),
            LabelledEndpointId::new_with_labels(
                &EndpointId::tty("/dev/ttyACMx"),
                &["device-type-1", "server-room-foo"],
            ),
        ])
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Sync(Sync::WriteOk) => write!(f, "Write ok"),
            Response::Sync(Sync::ObservingEvents) => write!(f, "User events subscription ok"),
            Response::Sync(Sync::Observing(id)) => write!(f, "Observing {id}"),
            Response::Sync(Sync::ControlQueue(ids)) => {
                write!(f, "In control queue for ")?;
                for id in ids {
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Response::Sync(Sync::ControlGranted(ids)) => {
                write!(f, "Control granted for ")?;
                for id in ids {
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Response::Async(Async::Message { endpoint, message }) => write!(
                f,
                "Message from {endpoint}: `[{:?}..]`",
                &message[..message.len().min(32)]
            ),
            Response::Async(Async::Event(event)) => write!(f, "UserEvent: `[{event}..]`",),
        }
    }
}

/// A fallible response.
pub type ResponseResult = Result<Response, error::Error>;
