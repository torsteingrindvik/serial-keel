use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    endpoint::{EndpointId, Label, LabelledEndpointId},
    error,
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

    /// Start controlling any endpoint matching the given label.
    ControlAny(Label),

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
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Control(e) => write!(f, "control: {e}"),
            Action::Observe(e) => write!(f, "observe: {e}"),
            Action::Write((e, msg)) => {
                write!(f, "write: {e}, msg: [{}]..", &msg[0..msg.len().min(16)])
            }
            Action::ControlAny(label) => write!(f, "control any: {label}"),
            Action::WriteBytes((e, bytes)) => {
                write!(
                    f,
                    "write: {e}, msg: [{:?}]..",
                    &bytes[0..bytes.len().min(16)]
                )
            }
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
        Self::Control(EndpointId::mock(name))
    }

    /// Create a control any action.
    pub fn control_any<S: AsRef<str>>(label: S) -> Self {
        Self::ControlAny(Label::new(label))
    }

    /// Create an observe action.
    pub fn observe(id: &EndpointId) -> Self {
        Self::Observe(id.clone())
    }

    /// Create an observe mock action.
    pub fn observe_mock(name: &str) -> Self {
        Self::Observe(EndpointId::mock(name))
    }

    /// Create a write action.
    pub fn write(id: &EndpointId, message: SerialMessage) -> Self {
        Self::Write((id.clone(), message))
    }

    /// Create a write bytes action.
    pub fn write_bytes(id: &EndpointId, bytes: SerialMessageBytes) -> Self {
        Self::WriteBytes((id.clone(), bytes))
    }

    /// Turn an action into serialized json.
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).expect("Should serialize well")
    }

    /// An example of a write message to a TTY endpoint.
    pub fn example_write() -> Self {
        Self::Write((EndpointId::tty("/dev/ttyACMx"), "This is a message".into()))
    }

    /// An example of writing bytes to a TTY endpoint.
    pub fn example_write_bytes() -> Self {
        Self::WriteBytes((
            EndpointId::tty("/dev/ttyACMx"),
            "This is a message".to_string().into_bytes(),
        ))
    }
}

// TODO: Split into either:
//
//  sync    - always as a response from the user
//  async   - might appear any time
//
//  or perhaps something like:
//  answer  - an answer to a request
//  data    - async stuff

/// Responses the server will send to connected users.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Response {
    /// The action was successful and no more context is needed.
    Ok,

    /// The requested endpoint was busy.
    /// When available, access is granted and
    /// [`Response::ControlGranted(_)`] is sent.
    ControlQueue(Vec<LabelledEndpointId>),

    /// The requested endpoint is now exclusively in use by the user.
    /// Writing to this endpoint is now possible.
    ControlGranted(Vec<LabelledEndpointId>),

    /// An endpoint sent a message.
    Message {
        /// Which endpoint sent a message.
        endpoint: LabelledEndpointId,

        /// The message contents.
        message: SerialMessageBytes,
    },
}

impl Response {
    /// An example of a control granted response.
    pub fn example_control_granted() -> Self {
        Self::ControlGranted(vec![
            LabelledEndpointId::new(&EndpointId::tty("COM0")),
            LabelledEndpointId::new_with_labels(
                &EndpointId::tty("/dev/ttyACMx"),
                &["device-type-1", "server-room-foo"],
            ),
        ])
    }

    /// An example of a new message response.
    pub fn example_new_message() -> Self {
        Self::Message {
            endpoint: LabelledEndpointId::new(&EndpointId::tty("COM0")),
            message: "Hello World!".into(),
        }
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Ok => write!(f, "Ok"),
            Response::ControlQueue(ids) => {
                write!(f, "In control queue for ")?;
                for id in ids {
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Response::ControlGranted(ids) => {
                write!(f, "Control granted for ")?;
                for id in ids {
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            Response::Message { endpoint, message } => write!(
                f,
                "Message from {endpoint}: `[{:?}..]`",
                &message[..message.len().min(32)]
            ),
        }
    }
}

/// A fallible response.
pub type ResponseResult = Result<Response, error::Error>;
