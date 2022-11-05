//! A mock endpoint.
//! Can be instructed to produce certain lines of output.
//! This is done via loopback.
//! So messages to put on the wire is instead sent back.
//!
//! Useful for testing implementations which would use
//! regular serial ports- but faster and more reliable.

use std::sync::Arc;

use futures::channel::mpsc;
use tokio::sync::{broadcast, Semaphore};

use super::Endpoint;
use crate::{mock::Mock, serial::serial_port::SerialMessage};

impl Endpoint for Mock {
    fn inbox(&self) -> broadcast::Receiver<SerialMessage> {
        self.broadcast_sender.subscribe()
    }

    fn semaphore(&self) -> Arc<Semaphore> {
        self.put_on_wire_permit.clone()
    }

    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessage> {
        self.should_put_on_wire_sender.clone()
    }

    fn label(&self) -> super::InternalEndpointLabel {
        super::InternalEndpointLabel::Mock(self.id.clone())
    }
}
