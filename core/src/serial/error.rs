use std::{io, string};

use thiserror::Error;
use tokio::sync::mpsc;

/// Any error this library might encounter.
#[derive(Debug, Error)]
pub enum SerialPortError {
    /// IO related errors.
    #[error("Underlying IO problem")]
    IO(#[from] io::Error),

    /// Utf8 related errors.
    #[error("Problem with UTF8 conversion")]
    Utf8(#[from] string::FromUtf8Error),

    /// Problem sending data.
    #[error("Problem sending data")]
    TrySend(#[from] mpsc::error::TrySendError<Vec<u8>>),

    /// Problem sending data.
    #[error("Problem sending data")]
    Send(#[from] mpsc::error::SendError<Vec<u8>>),

    /// Serial port disconnected.
    #[error("Serial port disconnected")]
    Disconnected,
}
