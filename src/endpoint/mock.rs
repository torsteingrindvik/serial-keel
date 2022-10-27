//! A mock endpoint.
//! Can be instructed to produce certain lines of output.
//! This is done via loopback.
//! So messages to put on the wire is instead sent back.
//!
//! Useful for testing implementations which would use
//! regular serial ports- but faster and more reliable.

use std::fmt::Display;

use futures::{channel::mpsc, StreamExt};
use nordic_types::serial::SerialMessage;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, info, warn};

use crate::user::User;

use super::Endpoint;

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) struct MockId {
    pub(crate) user: User,
    pub(crate) name: String,
}

impl Display for MockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.user.name, self.name)
    }
}

impl MockId {
    #[cfg(test)]
    pub(crate) fn new(user: &str, name: &str) -> Self {
        Self {
            user: User::new(user),
            name: name.into(),
        }
    }
}
pub(crate) struct Mock {
    id: MockId,

    // Used for giving out senders (via clone)
    should_put_on_wire_sender: mpsc::UnboundedSender<SerialMessage>,

    // Used for giving out receivers (via subscribe)
    broadcast_sender: broadcast::Sender<SerialMessage>,
}

impl Mock {
    pub(crate) fn run(mock_id: MockId) -> Self {
        info!(%mock_id, "Running mock");

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
                        for line in message.lines() {
                            match broadcast_sender_task.send(line.to_owned()) {
                                Ok(listeners) => {
                                    debug!("Broadcasted message to {listeners} listener(s)")
                                }
                                Err(e) => {
                                    warn!("Send error in broadcast: {e:?}")
                                }
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
            id: mock_id,
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

    fn label(&self) -> super::InternalEndpointLabel {
        super::InternalEndpointLabel::Mock(self.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, time::Duration};

    use super::*;

    use futures::SinkExt;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn loopback() {
        let mock = Mock::run(MockId::new("user", "mock"));

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        let to_send = "Hi";
        tx.send(to_send.into()).await.unwrap();

        let msg = rx.recv().await.unwrap();

        assert_eq!(to_send, msg.as_str());
    }

    #[tokio::test]
    async fn loopback_rx_created_late() {
        let mock = Mock::run(MockId::new("user2", "mock"));

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
        let mock = Mock::run(MockId::new("user3", "mock"));

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

    #[tokio::test]
    async fn newlines_are_split_up() {
        let mock = Mock::run(MockId::new("user4", "mock"));

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        tx.send(
            "This is a
message with a newline
or two."
                .into(),
        )
        .await
        .unwrap();

        for msg in ["This is a", "message with a newline", "or two."] {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg, &received);
        }
    }

    #[tokio::test]
    async fn newlines_from_embedded_file_are_split_up() {
        let mock = Mock::run(MockId::new("user5", "mock"));

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        let msg = include_str!("test-newlines.txt");

        tx.send(msg.into()).await.unwrap();

        for msg in [
            "this file should",
            "have some newlines",
            "and that should be reflected",
            "in",
            "the test",
        ] {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg, &received);
        }
    }

    #[tokio::test]
    async fn newlines_from_fs_file_are_split_up() {
        let mock = Mock::run(MockId::new("user6", "mock"));

        let mut tx = mock.outbox();
        let mut rx = mock.inbox();

        let msg = tokio::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint/test-newlines.txt"),
        )
        .await
        .unwrap();

        tx.send(msg).await.unwrap();

        for msg in [
            "this file should",
            "have some newlines",
            "and that should be reflected",
            "in",
            "the test",
        ] {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg, &received);
        }
    }
}
