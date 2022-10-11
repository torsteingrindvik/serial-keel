//! A mock endpoint.
//! Can be instructed to produce certain lines of output.
//! This is done via loopback.
//! So messages to put on the wire is instead sent back.
//!
//! Useful for testing implementations which would use
//! regular serial ports- but faster and more reliable.

use futures::{channel::mpsc, StreamExt};
use nordic_types::serial::SerialMessage;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, warn};

use super::Endpoint;

struct Mock {
    // Used for giving out senders (via clone)
    should_put_on_wire_sender: mpsc::UnboundedSender<SerialMessage>,

    // Used for giving out receivers (via subscribe)
    broadcast_sender: broadcast::Sender<SerialMessage>,
}

impl Mock {
    fn run() -> Self {
        // Listen to this internally.
        // If anything appears, put it on the broadcast.
        let (mpsc_sender, mpsc_receiver) = mpsc::unbounded();

        enum Event {
            PleasePutThisOnWire(SerialMessage),

            ThisCameFromWire(Option<SerialMessage>),
        }

        let messages_to_send_receiver = mpsc_receiver.map(Event::PleasePutThisOnWire);

        // Outsiders will be getting observing messages from this broadcast.
        let (broadcast_sender, broadcast_receiver) = broadcast::channel(1024);

        // We need a stream.
        let broadcast_receiver: BroadcastStream<SerialMessage> = broadcast_receiver.into();

        // We will discard problems.
        let broadcast_receiver = broadcast_receiver.map(|item| match item {
            Ok(message) => Event::ThisCameFromWire(Some(message)),
            Err(_) => Event::ThisCameFromWire(None),
        });

        let broadcast_sender_task = broadcast_sender.clone();

        tokio::spawn(async move {
            let mut events = futures::stream::select(messages_to_send_receiver, broadcast_receiver);

            loop {
                match events.select_next_some().await {
                    Event::PleasePutThisOnWire(message) => {
                        match broadcast_sender_task.send(message) {
                            Ok(listeners) => {
                                debug!("Broadcasted message to {listeners} listener(s)")
                            }
                            Err(e) => {
                                warn!("Send error in broadcast: {e:?}")
                            }
                        }
                    }
                    Event::ThisCameFromWire(Some(_message)) => {
                        // Nothing to do, we have already put it on the wire.
                    }
                    Event::ThisCameFromWire(None) => {
                        warn!("Problem in broadcast stream. Lagging receiver!");
                    }
                }
            }
        });

        Self {
            should_put_on_wire_sender: mpsc_sender,
            broadcast_sender,
        }
    }
}

impl Endpoint for Mock {
    fn inbox(&self) -> broadcast::Receiver<SerialMessage> {
        self.broadcast_sender.subscribe()
    }

    fn outbox(&self) -> mpsc::UnboundedSender<SerialMessage> {
        self.should_put_on_wire_sender.clone()
    }
    // fn handle(&self) -> super::EndpointHandle {
    //     EndpointHandle {
    //         arriving_messages: self.broadcast_sender.subscribe(),
    //         messages_to_send: Arc::new(Mutex::new(self.should_put_on_wire_sender.clone())),
    //     }
    // }
}

// impl Sink<SerialMessage> for Mock {
//     type Error = Infallible;

//     fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.dumpster.poll_ready_unpin(cx)
//     }

//     fn start_send(mut self: Pin<&mut Self>, item: SerialMessage) -> Result<(), Self::Error> {
//         self.dumpster.start_send_unpin(item)
//     }

//     fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.dumpster.poll_flush_unpin(cx)
//     }

//     fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.dumpster.poll_close_unpin(cx)
//     }
// }

// impl Stream for Mock {
//     type Item = SerialMessage;

//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         self.data.poll_next_unpin(cx)
//     }
// }

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    use futures::SinkExt;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn loopback() {
        let mock = Mock::run();

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        let to_send = "Hi";
        tx.send(to_send.into()).await.unwrap();

        let msg = rx.recv().await.unwrap();

        assert_eq!(to_send, msg.as_str());
    }

    #[tokio::test]
    async fn loopback_rx_created_late() {
        let mock = Mock::run();

        let mut tx = mock.outbox();

        // If we send before creating a receiver- will the message arrive?
        let to_send = "Hi";
        tx.send(to_send.into()).await.unwrap();

        // Gaurantee it has been sent
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut rx = mock.inbox();

        // It should not- the broadcast only gets things sent after subscribing.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn list_of_messages() {
        let mock = Mock::run();

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        let messages = ["one", "two", "three"];

        for msg in messages {
            tx.send(msg.into()).await.unwrap();
        }

        for msg in messages {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg, &received);
        }
    }
}
