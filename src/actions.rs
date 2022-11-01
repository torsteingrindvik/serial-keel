use nordic_types::serial::SerialMessage;
use serde::{Deserialize, Serialize};

use crate::{control_center::ControlCenterResponse, endpoint::EndpointLabel, error};

/// Actions user can ask of the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum Action {
    /// Start controlling the given endpoint.
    ///
    /// This allows reading output from the given endpoint,
    /// but also exclusively writing to the endpoint.
    /// Note that reading is implied- new lines will be sent
    /// to the user just like it would when observing.
    ///
    /// There may be many concurrent observers,
    /// but only a single controller.
    Control(EndpointLabel),

    /// Start observing the given endpoint.
    ///
    /// The user may only read output from the given endpoint.
    ///
    /// There may be many concurrent observers,
    /// but only a single controller.
    Observe(EndpointLabel),

    /// Put this message on the wire for the given endpoint.
    Write((EndpointLabel, SerialMessage)),
}

impl Action {
    /// Create an observe action.
    pub fn observe(label: &EndpointLabel) -> Self {
        Self::Observe(label.clone())
    }

    /// Create an observe mock action.
    pub fn observe_mock(name: &str) -> Self {
        Self::Observe(EndpointLabel::mock(name))
    }

    /// Create a write action.
    pub fn write(label: &EndpointLabel, message: SerialMessage) -> Self {
        Self::Write((label.clone(), message))
    }

    /// Turn an action into serialized json.
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).expect("Should serialize well")
    }
}

/// Responses the server will send to connected users.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Response {
    /// The action was successful and no more context is needed.
    Ok,

    /// An endpoint sent a message.
    Message {
        /// Which endpoint sent a message.
        endpoint: EndpointLabel,

        /// The message contents.
        message: String,
    },
}

/// A fallible response.
pub type ResponseResult = Result<Response, error::Error>;
