use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors thay may occur in this library.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    /// An internal problem.
    /// Should not show up in user code.
    #[error("Unexpected code path in library- likely bug!")]
    Bug,

    /// User sent a bad request.
    #[error("The request `{0}` could be be made into a request (bad JSON?)")]
    BadRequest(String),
}
