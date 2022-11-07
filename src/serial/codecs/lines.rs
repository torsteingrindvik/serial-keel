use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::serial::error::SerialPortError;

/// This codec has a configurable delimiter character for reading,
/// and optionally adds a character to each line it encodes.
#[derive(Debug)]
pub struct LinesCodec {
    /// How far we have looked for a newline into the buffer
    cursor: usize,

    /// How to delimit incoming byte streams.
    /// This delimiter is not included in the yielded frames.
    read_delimiter: u8,

    /// If provided, which byte to append when writing (encoding) messages.
    /// If `None`, forwards the data as-is.
    write_delimiter: Option<u8>,
}

impl LinesCodec {
    /// Create a new codec.
    pub fn new(read_delimiter: u8, write_delimiter: Option<u8>) -> Self {
        Self {
            cursor: 0,
            read_delimiter,
            write_delimiter,
        }
    }

    /// Return a [StringCodec], which does the same thing as the underlying [LinesCodec].
    /// The difference is that it writes strings instead of vectors of bytes.
    /// It also reads strings, and it is configurable whether bad utf8
    /// should result in an error, or be replaced with some lossy character.
    pub fn into_string_codec(self, lossy: bool) -> StringCodec {
        StringCodec {
            lossy,
            wrapped: self,
        }
    }
}

impl Default for LinesCodec {
    fn default() -> Self {
        Self::new(b'\n', None)
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
