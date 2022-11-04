//! A mock endpoint.
//! Can be instructed to produce certain lines of output.
//! This is done via loopback.
//! So messages to put on the wire is instead sent back.
//!
//! Useful for testing implementations which would use
//! regular serial ports- but faster and more reliable.

use std::hash::Hash;
use std::{fmt::Display, sync::Arc};

use futures::{channel::mpsc, StreamExt};
use nordic_types::serial::SerialMessage;
use tokio::sync::{broadcast, oneshot, Semaphore, TryAcquireError};
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, info, trace, warn};

use crate::user::User;

use super::{Endpoint, MaybeOutbox};

#[derive(Debug, Clone, Eq)]
#[cfg_attr(not(feature = "mocks-share-endpoints"), derive(Hash, PartialEq))]
pub(crate) struct MockId {
    pub(crate) user: User,
    pub(crate) name: String,
}

#[cfg(feature = "mocks-share-endpoints")]
impl Hash for MockId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // To enable shared mock endpoints
        // we hash only by mock name,
        // not by user.
        self.name.hash(state);
    }
}

#[cfg(feature = "mocks-share-endpoints")]
impl PartialEq for MockId {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Display for MockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.user.name, self.name)
    }
}

impl MockId {
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

    put_on_wire_permit: Arc<Semaphore>,
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
                                    trace!("Broadcasted message to {listeners} listener(s)")
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
            put_on_wire_permit: Arc::new(Semaphore::new(1)),
        }
    }
}

impl Endpoint for Mock {
    fn inbox(&self) -> broadcast::Receiver<SerialMessage> {
        self.broadcast_sender.subscribe()
    }

    fn outbox(&self) -> MaybeOutbox {
        match self.put_on_wire_permit.clone().try_acquire_owned() {
            Ok(permit) => MaybeOutbox::Available(super::Outbox {
                _permit: permit,
                inner: self.should_put_on_wire_sender.clone(),
            }),
            Err(TryAcquireError::NoPermits) => {
                let (permit_tx, permit_rx) = oneshot::channel();
                let permit_fut = self.put_on_wire_permit.clone().acquire_owned();
                let outbox = self.should_put_on_wire_sender.clone();

                tokio::spawn(async move {
                    if let Ok(permit) = permit_fut.await {
                        if permit_tx
                            .send(super::Outbox {
                                _permit: permit,
                                inner: outbox,
                            })
                            .is_err()
                        {
                            warn!("Permit acquired but no user to receive it")
                        };
                    } else {
                        warn!("Could not get permit- endpoint closed?")
                    }
                });

                MaybeOutbox::Busy(super::OutboxQueue(permit_rx))
            }
            Err(TryAcquireError::Closed) => unreachable!(),
        }
    }

    fn label(&self) -> super::InternalEndpointLabel {
        super::InternalEndpointLabel::Mock(self.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::endpoint::Outbox;
    use std::{env, path::PathBuf, time::Duration};

    use super::*;
    use futures::SinkExt;
    use pretty_assertions::assert_eq;

    fn available_outbox(mock: &Mock) -> Outbox {
        match mock.outbox() {
            MaybeOutbox::Available(o) => o,
            MaybeOutbox::Busy(_) => unreachable!(),
        }
    }

    #[tokio::test]
    async fn loopback() {
        let mock = Mock::run(MockId::new("user", "mock"));

        let mut tx = available_outbox(&mock);
        let mut rx = mock.inbox();

        let to_send = "Hi";
        tx.inner.send(to_send.into()).await.unwrap();

        let msg = rx.recv().await.unwrap();

        assert_eq!(to_send, msg.as_str());
    }

    #[tokio::test]
    async fn loopback_rx_created_late() {
        let mock = Mock::run(MockId::new("user2", "mock"));

        let mut tx = available_outbox(&mock);

        // If we send before creating a receiver- will the message arrive?
        let to_send = "Hi";
        tx.inner.send(to_send.into()).await.unwrap();

        // Gaurantee it has been sent
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut rx = mock.inbox();

        // It should not- the broadcast only gets things sent after subscribing.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn list_of_messages() {
        let mock = Mock::run(MockId::new("user3", "mock"));

        let mut tx = available_outbox(&mock);
        let mut rx = mock.inbox();

        let messages = ["one", "two", "three"];

        for msg in messages {
            tx.inner.send(msg.into()).await.unwrap();
        }

        for msg in messages {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg, &received);
        }
    }

    #[tokio::test]
    async fn newlines_are_split_up() {
        let mock = Mock::run(MockId::new("user4", "mock"));

        let mut tx = available_outbox(&mock);
        let mut rx = mock.inbox();

        tx.inner
            .send(
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

        let mut tx = available_outbox(&mock);
        let mut rx = mock.inbox();

        let msg = include_str!("test-newlines.txt");

        tx.inner.send(msg.into()).await.unwrap();

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

        let mut tx = available_outbox(&mock);
        let mut rx = mock.inbox();

        let msg = tokio::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoint/test-newlines.txt"),
        )
        .await
        .unwrap();

        tx.inner.send(msg).await.unwrap();

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

#[cfg(feature = "mocks-share-endpoints")]
#[cfg(test)]
mod shared_mocks {
    use crate::endpoint::mock::MockId;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn test_mocks_eq_when_users_eq() {
        let user = "foo";
        let endpoint = "mock";

        assert_eq!(MockId::new(user, endpoint), MockId::new(user, endpoint));
    }

    #[test]
    fn test_mocks_eq_when_users_ne() {
        let user_1 = "foo";
        let user_2 = "bar";

        let endpoint = "mock";

        // This is the difference: These are equal
        assert_eq!(MockId::new(user_1, endpoint), MockId::new(user_2, endpoint));
    }

    #[test]
    fn test_mocks_hash_eq_when_users_ne() {
        let user_1 = "foo";
        let user_2 = "bar";

        let endpoint = "mock";

        let mut h = HashSet::new();

        h.insert(MockId::new(user_1, endpoint));
        assert!(h.contains(&MockId::new(user_2, endpoint)));
    }
}

#[cfg(not(feature = "mocks-share-endpoints"))]
#[cfg(test)]
mod shared_mocks {
    use crate::endpoint::mock::MockId;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn test_mocks_eq_when_users_eq() {
        let user = "foo";
        let endpoint = "mock";

        assert_eq!(MockId::new(user, endpoint), MockId::new(user, endpoint));
    }

    #[test]
    fn test_mocks_ne_when_users_ne() {
        let user_1 = "foo";
        let user_2 = "bar";

        let endpoint = "mock";

        // This is the difference: These are **not** equal
        assert_ne!(MockId::new(user_1, endpoint), MockId::new(user_2, endpoint));
    }

    #[test]
    fn test_mocks_hash_ne_when_users_ne() {
        let user_1 = "foo";
        let user_2 = "bar";

        let endpoint = "mock";

        let mut h = HashSet::new();

        h.insert(MockId::new(user_1, endpoint));

        // Difference: Does NOT contain
        assert!(!h.contains(&MockId::new(user_2, endpoint)));
    }
}
