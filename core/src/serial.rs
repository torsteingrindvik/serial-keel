use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Serial port related errors.
pub(crate) mod error;

/// The serial port structure.
pub(crate) mod serial_port;

/// Codecs for encoding/decoding messages to/from wire.
pub(crate) mod codecs;

/// The message data type used for serial.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Deserialize, Serialize)]
pub struct SerialMessage(String);

impl SerialMessage {
    /// Create a serial message from bytes, ignoring any bad utf8 bytes.
    pub fn new_lossy<B: AsRef<[u8]>>(bytes: B) -> Self {
        Self(String::from_utf8_lossy(bytes.as_ref()).to_string())
    }

    /// Turn the message (utf8) into bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0.into_bytes()
    }

    /// Borrowed form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: AsRef<str>> From<T> for SerialMessage {
    fn from(string_like: T) -> Self {
        Self(string_like.as_ref().into())
    }
}

impl Display for SerialMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0.chars().take(48).collect::<String>();

        write!(f, "{}", s.trim())
    }
}

/// The message data type used for serial bytes.
pub type SerialMessageBytes = Vec<u8>;
