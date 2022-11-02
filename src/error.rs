use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors thay may occur in this library.
#[derive(Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum Error {
    /// An internal problem.
    /// Should not show up in user code.
    #[error("Unexpected code path in library- likely bug!")]
    Bug,

    /// User sent a bad request.
    #[error("The request led to a problem: {0}")]
    BadRequest(String),
}
