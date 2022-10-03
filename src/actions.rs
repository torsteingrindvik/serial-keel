use serde::{Deserialize, Serialize};

use crate::error;

/// Actions user can ask of the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum Action {}

impl TryFrom<ActionResponse> for Action {
    type Error = error::SerialKeelError;

    fn try_from(action_response: ActionResponse) -> Result<Self, Self::Error> {
        match action_response {
            ActionResponse::Action(action) => Ok(action),
            ActionResponse::Response(_) => Err(error::SerialKeelError::Bug),
        }
    }
}

/// Responses the server will send to connected users.
#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// The action was successful and no more context is needed.
    Ok,

    /// In case the user sent something which cannot be deserialized
    /// into [`Action`] from JSON.
    CouldNotDeserializeJsonToAction,
}

impl TryFrom<ActionResponse> for Response {
    type Error = error::SerialKeelError;

    fn try_from(action_response: ActionResponse) -> Result<Self, Self::Error> {
        match action_response {
            ActionResponse::Action(_) => Err(error::SerialKeelError::Bug),
            ActionResponse::Response(response) => Ok(response),
        }
    }
}

/// Helper enum for allowing having an action-response channel,
/// since both sender and receiver must be the same type.
#[derive(Debug)]
pub(crate) enum ActionResponse {
    Action(Action),
    Response(Response),
}

impl From<Action> for ActionResponse {
    fn from(action: Action) -> Self {
        Self::Action(action)
    }
}

impl From<Response> for ActionResponse {
    fn from(response: Response) -> Self {
        Self::Response(response)
    }
}
