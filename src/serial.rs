/// Serial port related errors.
pub(crate) mod error;

/// The serial port structure.
pub(crate) mod serial_port;

/// Codecs for encoding/decoding messages to/from wire.
pub(crate) mod codecs;

/// The message data type used for serial.
pub type SerialMessage = String;

/// The message data type used for serial bytes.
pub type SerialMessageBytes = Vec<u8>;
