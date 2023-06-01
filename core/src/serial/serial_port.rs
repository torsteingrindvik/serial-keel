#![allow(dead_code)] // TODO: Cleanup this module

use std::time::Duration;

use futures::{
    channel::mpsc::{self, UnboundedSender},
    StreamExt,
    channel::oneshot, SinkExt,
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
                error!(?e, "Serial port connection error. Retrying in 3 seconds...");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}


fn is_port_busy(path: String) -> bool {
    // Try to connect to the port
    let port = serialport::new(&path, 9600)
        .timeout(std::time::Duration::from_millis(10))
        .open();

    match port {
        Ok(_) => {
            // Huh, we are connected? This should not happen. We are not expecting to get connected to.
            warn!("Port {} could be connected to, but we did not expect it to be.", &path);
            return false;
        },
        Err(e) => {
            //If the error contains 'busy' or 'denied' (Windows), we are still connected
            if e.to_string().to_lowercase().contains("busy") || e.to_string().to_lowercase().contains("denied"){
                // If the port is 'busy', we are still connected.
                return true;
            } else {
                return false;
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
            None => { LinesCodec::default() },
            Some(line_codec) => { line_codec }
        };

        let handle = tokio::spawn(
            async move {
                loop {
                    let p_clone = path_clone.clone();
                    let serial_stream = loop_create_serial_port(baud, flow_control, p_clone.clone()).await;
                    let codec_clone = codec.clone();

                    // Sink: Send things (to serial port), stream: receive things (from serial port)
                    let (mut sink, stream) = codec_clone.framed(serial_stream).split();

                    let stream = stream.map(Event::ThisCameFromWire);

                    let mut events = futures::stream::select(stream, &mut * should_put_on_wire_receiver);

                    // We need to spawn a task here to poke the serial port to see if it's still alive.
                    // If it's not, we need to let the task below know so it can break and restart the connection. This task will also
                    // destruct and be recreated.

                    // One shot channel to tell the serial port task to break.
                    let (poke_serial_port_sender, mut poke_serial_port_receiver) = oneshot::channel::<bool>();

                    let poke_serial_port_task = tokio::spawn(async move {
                        loop {
                            // Wait for a bit before poking the serial port.
                            tokio::time::sleep(Duration::from_secs(4)).await;

                            // Try to connect to the port
                            if is_port_busy(p_clone.clone()) {
                                // We are still connected, so we can continue.
                                // If the port is not 'busy', we are disconnected.

                                continue;
                            } else {
                                poke_serial_port_sender.send(true).expect("Could not send to poke_serial_port_sender");
                                break;
                            }
                        }
                    });

                    loop {
                        futures::select! {
                            event = events.select_next_some() => {
                                match event {
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
                                            poke_serial_port_task.abort();
                                            break;
                                        }
                                    }
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
                                        poke_serial_port_task.abort();
                                        break;
                                    }
                                }
                            },
                            _ = poke_serial_port_receiver => {
                                // We got a message from the poke_serial_port_task, which means we are disconnected.
                                // We need to break out of this loop and restart the connection.
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
