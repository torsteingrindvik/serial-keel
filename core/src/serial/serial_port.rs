#![allow(dead_code)] // TODO: Cleanup this module

use futures::{
    channel::mpsc::{self, UnboundedSender},
    SinkExt, StreamExt,
};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;
use tracing::{error, info, info_span, trace, warn, Instrument};

use crate::{
    endpoint::{self, EndpointSemaphore, Label, Labels},
    error::Error,
    serial::{codecs::lines::LinesCodec, error::SerialPortError, SerialMessageBytes},
};

/// Builder for a [`SerialPortHandle`].
#[derive(Debug, Default)]
pub struct SerialPortBuilder {
    baud: Option<usize>,
    path: String,
    line_codec: Option<LinesCodec>,
    semaphore: Option<EndpointSemaphore>,
    labels: Labels,
}

impl SerialPortBuilder {
    /// Start a new builder.
    /// The tty should likely be along the lines of `/tty/ACMx` on unix, and `COMx` on Windows.
    pub(crate) fn new(tty: &str) -> Self {
        Self {
            path: tty.to_string(),
            ..Default::default()
        }
    }

    /// Set the [`EndpointSemaphore`] to use.
    pub(crate) fn set_semaphore(mut self, semaphore: EndpointSemaphore) -> Self {
        self.semaphore = Some(semaphore);
        self
    }

    /// Add a [`Label`].
    pub(crate) fn add_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    /// Set the [LinesCodec] to use.
    /// Will be ignored if [set_string_codec] has been called (so don't use both).
    pub(crate) fn set_line_codec(mut self, codec: LinesCodec) -> Self {
        self.line_codec = Some(codec);
        self
    }

    pub(crate) fn build(self) -> Result<SerialPortHandle, Error> {
        let baud = self.baud.unwrap_or(115_200) as u32;
        info!(%self.path, "Starting serial port handler");

        let serial_stream = tokio_serial::new(&self.path, baud)
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .flow_control(tokio_serial::FlowControl::Hardware)
            .open_native_async()
            .map_err(|e| {
                Error::InternalIssue(format!(
                    "Could not open port at {}, problem: {e:#?}",
                    self.path
                ))
            })?;

        let codec = if let Some(line_codec) = self.line_codec {
            line_codec
        } else {
            LinesCodec::default()
        };

        // Sink: Send things (to serial port), stream: receive things (from serial port)
        let (mut sink, stream) = codec.framed(serial_stream).split();

        enum Event {
            PleasePutThisOnWire(SerialMessageBytes),
            ThisCameFromWire(Result<SerialMessageBytes, SerialPortError>),
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
                        Event::PleasePutThisOnWire(message) => match sink.send(message.clone()).await {
                            Ok(()) => {
                                match broadcast_sender_task
                                    .send(endpoint::EndpointEvent::ToWire(message))
                                {
                                    Ok(listeners) => {
                                        trace!("Broadcasted ToWire message to {listeners} listener(s)")
                                    }
                                    Err(e) => {
                                        warn!("Send error in broadcast: {e:?}")
                                    }
                                }

                                continue;
                            }
                            Err(e) => {
                                error!(?e, "Serial port error in send, exiting");
                                break;
                            }
                        },
                        Event::ThisCameFromWire(Ok(message)) => {
                            trace!(
                                "Message from port: `{:?}`",
                                &message[..message.len().min(32)]
                            );

                            match broadcast_sender_task
                                .send(endpoint::EndpointEvent::FromWire(message))
                            {
                                Ok(listeners) => {
                                    trace!("Broadcasted FromWire to {listeners} listener(s)")
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

        Ok(SerialPortHandle {
            tty: self.path,
            handle,
            serial_tx: should_put_on_wire_sender,
            broadcast_tx: broadcast_sender,
            semaphore: self.semaphore.unwrap_or_default(),
            labels: self.labels,
        })
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
    pub(crate) serial_tx: UnboundedSender<SerialMessageBytes>,
    pub(crate) broadcast_tx: broadcast::Sender<endpoint::EndpointEvent>,
    pub(crate) semaphore: EndpointSemaphore,
    pub(crate) labels: Labels,
}
