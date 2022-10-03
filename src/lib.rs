#![deny(missing_docs)]

//! This crate sets up a serial port listener on the host machine.
//!
//! By default, the glob pattern `/dev/tty*` is watched.
//! If a port exists or appears, an agent is spawned for handling it.
//!
//! Users can subscriber to serial messages from a corresponding agent.
//!
//! Actions for mocking serial ports are available at a separate endpoint.
//! Using this unlocks some more actions.
//! For example, an action such as "inject this as the next response" is then available.
//! This allows a user to make the server reply with a specific response, which can be useful
//! to test logic without having the actual ports available.
//!
//! TODO:
//! - Watch the described glob pattern
//! - Serial port agents
//! - Subscription logic
//! - Mocking
//!
//! TESTS TODO:
//! - Subscription added, **several** messages are sent, but client is slow to fetch them. Messages lost?

/// The actions that can be sent from a connecting user,
/// as well as the responses.
pub mod actions;

/// Code relating to setting up a server.
pub mod server;

/// Possible errors in this library.
pub(crate) mod error;
