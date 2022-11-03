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

    /// The user did something which is not valid.
    /// For example, observe same endpoint twice.
    #[error("The request did not conform to valid usage. Problem: `{0}`")]
    BadUsage(String),
}
