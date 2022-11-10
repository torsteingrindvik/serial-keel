//! A serial port endpoint.
//! TODO

use futures::channel::mpsc;
use tokio::sync::broadcast;

use super::{Endpoint, EndpointSemaphore};
use crate::serial::serial_port::{SerialMessage, SerialPortHandle};

impl Endpoint for SerialPortHandle {
    fn inbox(&self) -> broadcast::Receiver<SerialMessage> {
        self.broadcast_tx.subscribe()
    }

    fn internal_endpoint_id(&self) -> super::InternalEndpointId {
        super::InternalEndpointId::Tty(self.tty.clone())
    }

    fn semaphore(&self) -> EndpointSemaphore {
        self.semaphore.clone()
    }

    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessage> {
        self.serial_tx.clone()
    }
}
