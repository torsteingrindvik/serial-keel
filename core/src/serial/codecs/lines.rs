use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::log::warn;

use crate::serial::error::SerialPortError;
use serialport;

/// This codec has a configurable delimiter character for reading,
/// and optionally adds a character to each line it encodes.
#[derive(Debug, Clone)]
pub struct LinesCodec {
    /// How far we have looked for a newline into the buffer
    cursor: usize,

    /// How to delimit incoming byte streams.
    /// This delimiter is not included in the yielded frames.
    read_delimiter: u8,

    /// If provided, which byte to append when writing (encoding) messages.
    /// If `None`, forwards the data as-is.
    write_delimiter: Option<u8>,

    serial_port_path: String,
}

impl LinesCodec {
    /// Create a new codec.
    pub fn new(read_delimiter: u8, write_delimiter: Option<u8>, path: String) -> Self {
        Self {
            cursor: 0,
            read_delimiter,
            write_delimiter,
            serial_port_path: path.clone(),
        }
    }
}

impl Default for LinesCodec {
    fn default() -> Self {
        Self::new(b'\n', None, String::from("/dev/ttyACM0"))
    }
}

impl Decoder for LinesCodec {
    type Item = Vec<u8>;
    type Error = SerialPortError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let read_to = src.len();

        let look_at = &src[self.cursor..read_to];

        if let Some(position) = look_at.iter().position(|&byte| byte == self.read_delimiter) {
            // Since we might "start late" in the buffer (from the cursor),
            // the "global" position within the buffer has to be calculated.
            let actual_position = self.cursor + position;

            // Next time we need to start over.
            self.cursor = 0;

            // Split at the delimiter, getting a slice of the bytes before it.
            let line = src.split_to(actual_position);

            // Discard the newline by advancing the source buffer beyond it.
            src.advance(1);

            Ok(Some(line[..].to_vec()))
        } else {
            // We did not find a full frame.
            // The next time we are called the same buffer `src` will be provided to us (same starting point),
            // but possibly with more data.
            // Since our job is to find the delimiter, we don't need to re-read the bytes we have already looked at.
            self.cursor = read_to;

            /* Handle disconnection */
            if look_at.is_empty() && read_to == 0 && self.cursor == 0 && src.len() == 0 {
                // This by itself is not enough to detect a disconnect, but it is a good indicator.
                // We need to try to connect to the port to be sure that the device has gone away.
                // If we get an 'busy' error, we are still connected.

                // Try to connect to the port
                let port = serialport::new(&self.serial_port_path, 9600)
                    .timeout(std::time::Duration::from_millis(10))
                    .open();
                match port {
                    Ok(_) => {
                        // Huh, we are connected? This should not happen. We are not expecting to get connected to.
                        warn!("Port {} could be connected to, but we did not expect it to be.", self.serial_port_path);
                    },
                    Err(e) => {
                        // If the error contains 'busy' or 'denied' (Windows), we are still connected
                        if e.to_string().to_lowercase().contains("busy") || e.to_string().to_lowercase().contains("denied"){
                            // If the port is 'busy', we are still connected.
                            return Ok(None);
                        } else {
                            // If the port is not 'busy', we are disconnected.
                            return Err(SerialPortError::Disconnected);
                        }
                    }
                }
            }

            // Indicate that we need more bytes to look at.
            Ok(None)
        }
    }
}

impl Encoder<Vec<u8>> for LinesCodec {
    type Error = SerialPortError;

    fn encode(&mut self, item: Vec<u8>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&item);

        if let Some(character) = self.write_delimiter {
            dst.extend_from_slice(&[character]);
        }
        Ok(())
    }
}

/// This does the same thing as the underlying [LinesCodec].
/// The difference is that it reads strings, and it is configurable whether bad utf8
/// should result in an error, or be replaced with some lossy character.
///
/// It can write and read strings.
#[derive(Debug)]
pub struct StringCodec {
    lossy: bool,
    wrapped: LinesCodec,
}

impl Decoder for StringCodec {
    type Item = String;
    type Error = SerialPortError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.wrapped.decode(src)? {
            Some(bytes) => {
                if self.lossy {
                    Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
                } else {
                    Ok(Some(String::from_utf8(bytes)?))
                }
            }
            None => Ok(None),
        }
    }
}

impl Encoder<String> for StringCodec {
    type Error = SerialPortError;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(item.as_bytes());
        Ok(())
    }
}

impl Encoder<Vec<u8>> for StringCodec {
    type Error = SerialPortError;

    fn encode(&mut self, item: Vec<u8>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.wrapped.encode(item, dst)
    }
}
