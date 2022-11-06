use std::sync::Arc;

use crate::serial::{
    codecs::lines::{LinesCodec, StringCodec},
    error::SerialPortError,
};
use futures::{
    channel::mpsc::{self, UnboundedSender},
    SinkExt, StreamExt,
};
use tokio::{
    sync::{broadcast, Semaphore},
    task::JoinHandle,
};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;
use tracing::{error, info, info_span, trace, warn, Instrument};

/// The message data type used for serial.
pub type SerialMessage = String;

/// The message data type used for serial bytes.
pub type SerialMessageBytes = Vec<u8>;

/// Builder for a [SerialPortHandle].
#[derive(Debug, Default)]
pub struct SerialPortBuilder {
    baud: Option<usize>,
    path: String,
    line_codec: Option<LinesCodec>,
    string_codec: Option<StringCodec>,

    lossy_utf8: bool,
}

impl SerialPortBuilder {
    /// Start a new builder.
    /// The tty should likely be along the lines of `/tty/ACMx` on unix, and `COMx` on Windows.
    pub(crate) fn new(tty: &str) -> Self {
        Self {
            path: tty.to_string(),
            lossy_utf8: true,
            ..Default::default()
        }
    }

    /// Set the [StringCodec] to use.
    /// Will take precedence over [LinesCodec] (so don't use both).
    pub(crate) fn set_string_codec(mut self, codec: StringCodec) -> Self {
        self.string_codec = Some(codec);
        self
    }

    /// Set the [LinesCodec] to use.
    /// Will be ignored if [set_string_codec] has been called (so don't use both).
    pub(crate) fn set_line_codec(mut self, codec: LinesCodec) -> Self {
        self.line_codec = Some(codec);
        self
    }

    /// Ignore bad utf8 (default, `true`), or promote it to errors (`false`).
    pub(crate) fn set_lossy_utf8(mut self, ignore: bool) -> Self {
        self.lossy_utf8 = ignore;
        self
    }

    #[must_use]
    pub(crate) fn build(self) -> SerialPortHandle {
        let baud = self.baud.unwrap_or(115_200) as u32;

        let serial_stream = tokio_serial::new(&self.path, baud)
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .flow_control(tokio_serial::FlowControl::Hardware)
            .open_native_async()
            .expect("Not being able to open serial port is non-recoverable");

        let codec = if let Some(string_codec) = self.string_codec {
            string_codec
        } else if let Some(line_codec) = self.line_codec {
            line_codec.to_string_codec(self.lossy_utf8)
        } else {
            LinesCodec::default().to_string_codec(self.lossy_utf8)
        };

        // Sink: Send things (to serial port), stream: receive things (from serial port)
        let (mut sink, stream) = codec.framed(serial_stream).split();

        enum Event {
            PleasePutThisOnWire(SerialMessage),
            ThisCameFromWire(Result<SerialMessage, SerialPortError>),
        }

        let stream = stream.map(Event::ThisCameFromWire);

        let (should_put_on_wire_sender, should_put_on_wire_receiver) = mpsc::unbounded();
        let should_put_on_wire_receiver =
            should_put_on_wire_receiver.map(Event::PleasePutThisOnWire);

        // Outsiders will be getting observing messages from this broadcast.
        let (broadcast_sender, broadcast_receiver) = broadcast::channel(1024);

        let broadcast_sender_task = broadcast_sender.clone();

        let tty_span = info_span!("tty", %self.path);

        let handle = tokio::spawn(
            async move {
                let mut events = futures::stream::select(stream, should_put_on_wire_receiver);

                loop {
                    match events.select_next_some().await {
                        Event::PleasePutThisOnWire(message) => match sink.send(message).await {
                            Ok(()) => continue,
                            Err(e) => {
                                error!(?e, "Serial port error in send, exiting");
                                break;
                            }
                        },
                        Event::ThisCameFromWire(Ok(message)) => {
                            trace!("Message from port: `{}`", &message[..message.len().min(32)]);

                            match broadcast_sender_task.send(message) {
                                Ok(listeners) => {
                                    trace!("Broadcasted message to {listeners} listener(s)")
                                }
                                Err(e) => {
                                    warn!("Send error in broadcast: {e:?}")
                                }
                            }
                        }
                        Event::ThisCameFromWire(Err(e)) => {
                            error!(?e, "Serial port error, exiting");
                            break;
                        }
                    }
                }

                // Just to make sure it lives as long as the serial port does.
                drop(broadcast_receiver);
            }
            .instrument(tty_span),
        );

        SerialPortHandle {
            tty: self.path,
            handle,
            serial_tx: should_put_on_wire_sender,
            broadcast_tx: broadcast_sender,
            put_on_wire_permit: Arc::new(Semaphore::new(1)),
        }
    }

    /// Set the serial port builder's baud.
    /// Will use 115_200 if not set.
    pub fn set_baud(&mut self, baud: usize) {
        self.baud = Some(baud);
    }
}

pub(crate) struct SerialPortHandle {
    pub(crate) tty: String,
    pub(crate) handle: JoinHandle<()>,
    pub(crate) serial_tx: UnboundedSender<SerialMessage>,
    pub(crate) broadcast_tx: broadcast::Sender<SerialMessage>,
    pub(crate) put_on_wire_permit: Arc<Semaphore>,
}