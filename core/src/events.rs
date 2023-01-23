use crate::{endpoint::InternalEndpointInfo, serial::SerialMessage, user::User};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{self, collections::VecDeque, fmt::Display};
use tokio::sync::broadcast;
use tracing::info;

/// These events are not necessarily tied to any user.
pub mod general {
    use super::*;

    /// Events that can happen to a user.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub enum Event {
        /// A message was sent (i.e. put on wire).
        MessageSent((InternalEndpointInfo, SerialMessage)),
        /// A message was received (i.e. from wire).
        MessageReceived((InternalEndpointInfo, SerialMessage)),
    }

    impl Display for Event {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Event::MessageSent((endpoint, msg)) => write!(f, "sent: {msg} to {endpoint}"),
                Event::MessageReceived((endpoint, msg)) => {
                    write!(f, "received: {msg} from {endpoint}")
                }
            }
        }
    }
}

/// These events relate to some user.
pub mod user {
    use super::*;

    /// An event related to some user.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct UserEvent {
        /// The user related to this event.
        pub user: User,

        /// The event.
        pub event: Event,
    }

    impl Display for UserEvent {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}: {}", self.user, self.event)
        }
    }

    /// Events that can happen to a user.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub enum Event {
        /// A user has connected.
        Connected,

        /// A user has disconnected.
        Disconnected,

        /// A user sent (i.e. put on wire) this message.
        MessageSent((InternalEndpointInfo, SerialMessage)),
        /// A user received (i.e. got from wire) this message.
        MessageReceived((InternalEndpointInfo, SerialMessage)),

        /// A user is now observing some endpoints.
        Observing(Vec<InternalEndpointInfo>),

        /// A user is no longer observing some endpoints.
        NoLongerObserving(Vec<InternalEndpointInfo>),

        /// A user is now in queue for some endpoints.
        InQueueFor(Vec<InternalEndpointInfo>),

        /// A user is now in control of some endpoints.
        InControlOf(Vec<InternalEndpointInfo>),

        /// A user is no longer in queue for some endpoints.
        NoLongerInQueueOf(Vec<InternalEndpointInfo>),

        /// A user is no longer in control of some endpoints.
        NoLongerInControlOf(Vec<InternalEndpointInfo>),
    }

    impl Display for Event {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut write_endpoints = |prefix: &str, endpoints: &[InternalEndpointInfo]| {
                let endpoints = endpoints
                    .iter()
                    .map(|endpoint| format!("{}", endpoint))
                    .join(", ");
                write!(f, "{prefix} {}", endpoints)
            };

            match self {
                Event::Connected => write!(f, "connected"),
                Event::Disconnected => write!(f, "disconnected"),
                Event::Observing(endpoints) => write_endpoints("observing", endpoints),
                Event::NoLongerObserving(endpoints) => {
                    write_endpoints("no longer observing", endpoints)
                }
                Event::InQueueFor(endpoints) => write_endpoints("in queue for", endpoints),
                Event::InControlOf(endpoints) => write_endpoints("in control of", endpoints),
                Event::NoLongerInQueueOf(endpoints) => {
                    write_endpoints("no longer in queue for", endpoints)
                }
                Event::NoLongerInControlOf(endpoints) => {
                    write_endpoints("no longer in control of", endpoints)
                }
                Event::MessageSent((info, msg)) => write!(f, "sent: {msg} to {info}"),
                Event::MessageReceived((info, msg)) => write!(f, "received: {msg} to {info}"),
            }
        }
    }
}

/// Any event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Event {
    /// An event tied to a user.
    User(user::UserEvent),

    /// An event not tied to a user.
    General(general::Event),
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::User(ue) => write!(f, "{ue}"),
            Event::General(e) => write!(f, "{e}"),
        }
    }
}

/// An event related to some user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimestampedEvent {
    /// The event.
    pub inner: Event,

    /// When the event happened.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl TimestampedEvent {
    /// Create a new user event.
    pub fn new_user_event(user: &User, event: user::Event) -> Self {
        Self {
            inner: Event::User(user::UserEvent {
                user: user.clone(),
                event,
            }),
            timestamp: chrono::Utc::now(),
        }
    }
}

impl Display for TimestampedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

/// An event logger and broadcaster.
#[derive(Debug)]
pub struct Events {
    log: VecDeque<TimestampedEvent>,
    log_size: usize,

    tx: broadcast::Sender<TimestampedEvent>,
    #[allow(dead_code)]
    rx: broadcast::Receiver<TimestampedEvent>,
}

impl Events {
    /// Create a new events handler.
    /// It will keep a log of at most `log_size` events.
    /// It may be subscribed to to receive any events it sees.
    pub fn new(log_size: usize) -> Self {
        let (tx, rx) = broadcast::channel(100);
        Self {
            tx,
            rx,
            log: VecDeque::new(),
            log_size,
        }
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<TimestampedEvent> {
        self.tx.subscribe()
    }

    /// Send an event. This will append it to the log and broadcast it to any subscribers.
    pub fn send_event(&mut self, event: TimestampedEvent) {
        info!(%event, "Sending and storing event");
        self.log.push_front(event.clone());

        // Keep a log of at most this number recent events.
        // Truncate removes from the back, which means older events are split off first.
        self.log.truncate(self.log_size);

        self.tx.send(event).expect("Broadcast should work");
    }

    /// Send a user event. See [`send_event`].
    pub fn send_user_event(&mut self, user: &User, event: user::Event) {
        self.send_event(TimestampedEvent::new_user_event(user, event))
    }
}
