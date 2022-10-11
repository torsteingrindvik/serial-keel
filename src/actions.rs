use nordic_types::serial::SerialMessage;
use serde::{Deserialize, Serialize};

use crate::{endpoint::EndpointLabel, error};

/// Actions user can ask of the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum Action {
    /// Start observing the given endpoint.
    Observe(EndpointLabel),

    /// Put this message on the wire for the given endpoint.
    Write((EndpointLabel, SerialMessage)),

    /// Create a mocked endpoint.
    /// After creation, it can then be observed.
    CreateMockEndpoint {
        /// The endpoint's name.
        /// After creation, it can be referred to via [`Endpoint::Mock`].
        name: String,
    },
}

impl Action {
    /// Turn an action into serialized json.
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).expect("Should serialize well")
    }

    /// Make a mock endpoint with this name
    pub(crate) fn create_mock(name: &str) -> Self {
        Self::CreateMockEndpoint { name: name.into() }
    }
}

impl TryFrom<ActionResponse> for Action {
    type Error = error::Error;

    fn try_from(action_response: ActionResponse) -> Result<Self, Self::Error> {
        match action_response {
            ActionResponse::Action(action) => Ok(action),
            ActionResponse::Response(_) => Err(error::Error::Bug),
        }
    }
}

/// Responses the server will send to connected users.
#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// The action was successful and no more context is needed.
    Ok,
}

impl TryFrom<ActionResponse> for ResponseResult {
    type Error = error::Error;

    fn try_from(action_response: ActionResponse) -> Result<Self, Self::Error> {
        match action_response {
            ActionResponse::Action(_) => Err(error::Error::Bug),
            ActionResponse::Response(response) => Ok(response),
        }
    }
}

/// A fallible response.
pub type ResponseResult = Result<Response, error::Error>;

/// Helper enum for allowing having an action-response channel,
/// since both sender and receiver must be the same type.
#[derive(Debug)]
pub(crate) enum ActionResponse {
    Action(Action),
    Response(ResponseResult),
}

impl From<Action> for ActionResponse {
    fn from(action: Action) -> Self {
        Self::Action(action)
    }
}

impl From<Response> for ActionResponse {
    fn from(response: Response) -> Self {
        Self::Response(Ok(response))
    }
}

impl From<error::Error> for ActionResponse {
    fn from(e: error::Error) -> Self {
        Self::Response(Err(e))
    }
}
