//! A serial port endpoint.
//! TODO

use tokio::sync::broadcast;

use super::Endpoint;
use crate::serial::serial_port::{SerialMessage, SerialPortHandle};

impl Endpoint for SerialPortHandle {
    fn inbox(&self) -> broadcast::Receiver<SerialMessage> {
        self.broadcast_tx.subscribe()
    }

    fn label(&self) -> super::InternalEndpointLabel {
        super::InternalEndpointLabel::Tty(super::Tty::new(&self.tty))
    }

    fn semaphore(&self) -> std::sync::Arc<tokio::sync::Semaphore> {
        self.put_on_wire_permit.clone()
    }

    fn message_sender(&self) -> futures::channel::mpsc::UnboundedSender<SerialMessage> {
        self.serial_tx.clone()
    }
}
