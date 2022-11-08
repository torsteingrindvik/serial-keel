use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors thay may occur in this library.
#[derive(Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    /// Endpoint does not exist.
    #[error("The endpoint `{0}` does not exist")]
    NoSuchEndpoint(String),

    /// Bad json.
    #[error("The request `{request}` could not be deserialized. Problem: {problem}")]
    BadJson {
        /// The problematic request.
        request: String,

        /// The deserialization issue.
        problem: String,
    },

    /// User tried to perform something which requires permission
    /// without having that permission first.
    #[error("No permit: {0}")]
    NoPermit(String),

    /// The user asked for more than what was needed.
    /// For example, observe same endpoint twice.
    #[error("The request was superfluous. Problem: `{0}`")]
    SuperfluousRequest(String),

    /// The user did something which is not valid.
    #[error("The request did not conform to valid usage. Problem: `{0}`")]
    BadUsage(String),

    /// Configuration file problems.
    #[error("The server configuration is not valid. Problem: `{0}`")]
    BadConfig(String),
}

impl Error {
    /// Try coercing this error into a bad config error.
    pub fn try_into_bad_config(self) -> Result<String, Self> {
        if let Self::BadConfig(v) = self {
            Ok(v)
        } else {
            Err(self)
        }
    }
}
