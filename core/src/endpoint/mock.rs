//! A mock endpoint.
//! Can be instructed to produce certain lines of output.
//! This is done via loopback.
//! So messages to put on the wire is instead sent back.
//!
//! Useful for testing implementations which would use
//! regular serial ports- but faster and more reliable.

use futures::channel::mpsc;
use tokio::sync::broadcast;

use super::{Endpoint, EndpointSemaphore};
use crate::{mock::MockHandle, serial::SerialMessageBytes};

impl Endpoint for MockHandle {
    fn events(&self) -> broadcast::Receiver<super::EndpointEvent> {
        self.broadcast_sender.subscribe()
    }

    fn semaphore(&self) -> EndpointSemaphore {
        self.semaphore.clone()
    }

    fn message_sender(&self) -> mpsc::UnboundedSender<SerialMessageBytes> {
        self.should_put_on_wire_sender.clone()
    }

    fn internal_endpoint_id(&self) -> super::InternalEndpointId {
        super::InternalEndpointId::Mock(self.id.clone())
    }

    fn labels(&self) -> Option<super::Labels> {
        self.labels.clone()
    }
}
