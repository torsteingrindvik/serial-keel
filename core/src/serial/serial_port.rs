#![allow(dead_code)] // TODO: Cleanup this module

use std::time::Duration;

use futures::{
    channel::mpsc::{self, UnboundedSender},
    SinkExt, StreamExt,
};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_serial::{SerialPortBuilderExt, SerialStream};
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
    flow_control: Option<serialport::FlowControl>,
    path: String,
    line_codec: Option<LinesCodec>,
    semaphore: Option<EndpointSemaphore>,
    labels: Labels,
}

fn try_create_serial_port(baud: u32, flow_control: serialport::FlowControl, path: String) -> Result<SerialStream, Error> {
    let serial_stream = tokio_serial::new(&path, baud)
    .data_bits(tokio_serial::DataBits::Eight)
    .parity(tokio_serial::Parity::None)
    .stop_bits(tokio_serial::StopBits::One)
    .flow_control(flow_control)
    .open_native_async()
    .map_err(|e| {
        Error::InternalIssue(format!(
            "Could not open port at {}, problem: {e:#?}",
            path
        ))
    });

    serial_stream
}

async fn loop_create_serial_port(baud: u32, flow_control: serialport::FlowControl, path: String) -> SerialStream {
    info!("Attempting to connect to serial port at {}", path);
    loop {
        match try_create_serial_port(baud, flow_control, path.clone()) {
            Ok(serial_stream) => {
                info!("Connected to serial port at {}", path);
                return serial_stream;
            }
            Err(e) => {
                error!(?e, "Serial port connection error. Retrying in 5 seconds...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
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
        let flow_control = self.flow_control.unwrap_or(serialport::FlowControl::None);

        info!(%self.path, %baud, ?flow_control, "Starting serial port handler");

        // Check here and return an error if we can't open the port right away.
        try_create_serial_port(baud, flow_control, self.path.clone())?;

        enum Event {
            PleasePutThisOnWire(SerialMessageBytes),
            ThisCameFromWire(Result<SerialMessageBytes, SerialPortError>),
        }

        let (should_put_on_wire_sender, should_put_on_wire_receiver) = mpsc::unbounded();

        let mut should_put_on_wire_receiver =
            Box::pin(should_put_on_wire_receiver.map(Event::PleasePutThisOnWire));

        // Outsiders will be getting observing messages from this broadcast.
        #[allow(unused_variables)]
        let (broadcast_sender, broadcast_receiver) = broadcast::channel(1024);

        let broadcast_sender_task = broadcast_sender.clone();

        let tty_span = info_span!("tty", %self.path);

        let path_clone = self.path.clone();
        let codec = match self.line_codec {
            None => { LinesCodec::new(b'\n', None, path_clone.clone()) },
            Some(line_codec) => { line_codec }
        };

        let handle = tokio::spawn(
            async move {
                loop {
                    let serial_stream = loop_create_serial_port(baud, flow_control, path_clone.clone()).await;
                    let codec_clone = codec.clone();

                    // Sink: Send things (to serial port), stream: receive things (from serial port)
                    let (mut sink, stream) = codec_clone.framed(serial_stream).split();

                    let stream = stream.map(Event::ThisCameFromWire);

                    let mut events = futures::stream::select(stream, &mut * should_put_on_wire_receiver);

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
                                match broadcast_sender_task
                                    .send(endpoint::EndpointEvent::SerialPortDisconnected())
                                {
                                    Ok(listeners) => {
                                        trace!("Broadcasted Error to {listeners} listener(s)")
                                    }
                                    Err(e) => {
                                        warn!("Send error in broadcast: {e:?}")
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
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

    pub fn set_flow_control(&mut self, flow_control: serialport::FlowControl) {
        self.flow_control = Some(flow_control);
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
