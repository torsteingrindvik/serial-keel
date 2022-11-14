//! A mock, useful to test serial port functionality without the actual serial ports.

use std::fmt::Display;
use std::hash::Hash;

use futures::{channel::mpsc, StreamExt};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{info, trace, warn};

use crate::{
    endpoint::{EndpointSemaphore, Label},
    serial::SerialMessageBytes,
    user::User,
};

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

/// Builder for a [`MockHandle`].
#[derive(Debug)]
pub(crate) struct MockBuilder {
    mock_id: MockId,
    semaphore: Option<EndpointSemaphore>,
    labels: Option<Vec<Label>>,
}

impl MockBuilder {
    pub(crate) fn new(mock_id: MockId) -> Self {
        Self {
            mock_id,
            semaphore: None,
            labels: None,
        }
    }

    /// Set the [`EndpointSemaphore`] to use.
    pub(crate) fn set_semaphore(mut self, semaphore: EndpointSemaphore) -> Self {
        self.semaphore = Some(semaphore);
        self
    }

    /// Add a [`Label`].
    pub(crate) fn add_label(mut self, label: Label) -> Self {
        self.labels.get_or_insert(vec![]).push(label);
        self
    }

    #[must_use]
    pub(crate) fn build(self) -> MockHandle {
        info!(%self.mock_id, "Running mock");

        // Listen to this internally.
        // If anything appears, put it on the broadcast.
        let (should_put_on_wire_sender, should_put_on_wire_receiver) = mpsc::unbounded();

        enum Event {
            PleasePutThisOnWire(SerialMessageBytes),
            ThisCameFromWire(Option<SerialMessageBytes>),
        }

        let messages_to_send_receiver = should_put_on_wire_receiver.map(Event::PleasePutThisOnWire);

        // Outsiders will be getting observing messages from this broadcast.
        let (broadcast_sender, broadcast_receiver) = broadcast::channel(1024);

        // We need a stream.
        let broadcast_receiver: BroadcastStream<SerialMessageBytes> = broadcast_receiver.into();

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
                        let message = String::from_utf8_lossy(&message);

                        let newlines = message.chars().filter(|c| c == &'\n').count();
                        trace!(
                            "Got message of length {} with #{newlines} newlines",
                            message.len()
                        );
                        for line in message.lines() {
                            match broadcast_sender_task.send(line.to_owned().into_bytes()) {
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

        MockHandle {
            should_put_on_wire_sender,
            broadcast_sender,
            id: self.mock_id,
            semaphore: self.semaphore.unwrap_or_default(),
            labels: self.labels,
        }
    }
}

pub(crate) struct MockHandle {
    pub(crate) id: MockId,

    // Used for giving out senders (via clone)
    pub(crate) should_put_on_wire_sender: mpsc::UnboundedSender<SerialMessageBytes>,

    // Used for giving out receivers (via subscribe)
    pub(crate) broadcast_sender: broadcast::Sender<SerialMessageBytes>,

    pub(crate) semaphore: EndpointSemaphore,

    pub(crate) labels: Option<Vec<Label>>,
}

// impl MockHandle {
// pub(crate) fn run_with_semaphore(mock_id: MockId, semaphore: EndpointSemaphore) -> Self {
//     info!(%mock_id, "Running mock");

//     // Listen to this internally.
//     // If anything appears, put it on the broadcast.
//     let (should_put_on_wire_sender, should_put_on_wire_receiver) = mpsc::unbounded();

//     enum Event {
//         PleasePutThisOnWire(SerialMessage),
//         ThisCameFromWire(Option<SerialMessage>),
//     }

//     let messages_to_send_receiver = should_put_on_wire_receiver.map(Event::PleasePutThisOnWire);

//     // Outsiders will be getting observing messages from this broadcast.
//     let (broadcast_sender, broadcast_receiver) = broadcast::channel(1024);

//     // We need a stream.
//     let broadcast_receiver: BroadcastStream<SerialMessage> = broadcast_receiver.into();

//     // We will discard problems.
//     let broadcast_receiver = broadcast_receiver.map(|item| match item {
//         Ok(message) => Event::ThisCameFromWire(Some(message)),
//         Err(_) => Event::ThisCameFromWire(None),
//     });

//     let broadcast_sender_task = broadcast_sender.clone();

//     tokio::spawn(async move {
//         let mut events = futures::stream::select(messages_to_send_receiver, broadcast_receiver);

//         loop {
//             match events.select_next_some().await {
//                 Event::PleasePutThisOnWire(message) => {
//                     let newlines = message.chars().filter(|c| c == &'\n').count();
//                     debug!(
//                         "Got message of length {} with #{newlines} newlines",
//                         message.len()
//                     );
//                     for line in message.lines() {
//                         match broadcast_sender_task.send(line.to_owned()) {
//                             Ok(listeners) => {
//                                 trace!("Broadcasted message to {listeners} listener(s)")
//                             }
//                             Err(e) => {
//                                 warn!("Send error in broadcast: {e:?}")
//                             }
//                         }
//                     }
//                 }
//                 Event::ThisCameFromWire(Some(_message)) => {
//                     // Nothing to do, we have already put it on the wire.
//                 }
//                 Event::ThisCameFromWire(None) => {
//                     warn!("Problem in broadcast stream. Lagging receiver!");
//                 }
//             }
//         }
//     });

//     Self {
//         should_put_on_wire_sender,
//         broadcast_sender,
//         id: mock_id,
//         semaphore,
//         labels: todo!(),
//     }
// }

// pub(crate) fn run(mock_id: MockId) -> Self {
//     Self::run_with_semaphore(mock_id, EndpointSemaphore::default())
// }
// }

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, time::Duration};

    use futures::SinkExt;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::endpoint::Endpoint;

    #[tokio::test]
    async fn loopback() {
        let mock = MockBuilder::new(MockId::new("user", "mock")).build();

        let mut tx = mock.message_sender();
        let mut rx = mock.inbox();

        let to_send = "Hi";
        tx.send(to_send.into()).await.unwrap();

        let msg = rx.recv().await.unwrap();

        assert_eq!(to_send.as_bytes(), msg);
    }

    #[tokio::test]
    async fn loopback_rx_created_late() {
        let mock = MockBuilder::new(MockId::new("user2", "mock")).build();

        let mut tx = mock.message_sender();

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
        let mock = MockBuilder::new(MockId::new("user3", "mock")).build();

        let mut tx = mock.message_sender();
        let mut rx = mock.inbox();

        let messages = ["one", "two", "three"];

        for msg in messages {
            tx.send(msg.into()).await.unwrap();
        }

        for msg in messages {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg.as_bytes(), &received);
        }
    }

    #[tokio::test]
    async fn newlines_are_split_up() {
        let mock = MockBuilder::new(MockId::new("user4", "mock")).build();

        let mut tx = mock.message_sender();
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
            assert_eq!(msg.as_bytes(), &received);
        }
    }

    #[tokio::test]
    async fn newlines_from_embedded_file_are_split_up() {
        let mock = MockBuilder::new(MockId::new("user5", "mock")).build();

        let mut tx = mock.message_sender();
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
            assert_eq!(msg.as_bytes(), &received);
        }
    }

    #[tokio::test]
    async fn newlines_from_fs_file_are_split_up() {
        let mock = MockBuilder::new(MockId::new("user6", "mock")).build();

        let mut tx = mock.message_sender();
        let mut rx = mock.inbox();

        let msg = tokio::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/test-newlines.txt"),
        )
        .await
        .unwrap();

        tx.send(msg.into_bytes()).await.unwrap();

        for msg in [
            "this file should",
            "have some newlines",
            "and that should be reflected",
            "in",
            "the test",
        ] {
            let received = rx.recv().await.unwrap();
            assert_eq!(msg.as_bytes(), &received);
        }
    }
}

#[cfg(feature = "mocks-share-endpoints")]
#[cfg(test)]
mod shared_mocks {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use crate::mock::MockId;

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
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use crate::mock::MockId;

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
