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
//! - Make a "Tty bundle" for grouping ttys.
//!    * They must be controlled together, so they should share a semaphore
//!    * We should communicate to the user that several endpoints are available
//!
//! TESTS TODO:
//! - Subscription added, **several** messages are sent, but client is slow to fetch them. Messages lost?

/// The actions that can be sent from a connecting user,
/// as well as the responses.
pub mod actions;

/// Code relating to setting up the server which sets up connections and spawns handlers for clients.
pub mod server;

/// A client.
pub mod client;

/// The command line interface.
pub mod cli;

/// Runs on the server.
///
/// Checks for new serial ports to listen to.
/// Sets up mock endpoints on demand.
pub(crate) mod control_center;

/// Handles incoming websockets.
pub(crate) mod websocket;

/// The actor spawned from a connected user.
pub(crate) mod peer;

/// Mocked serial port driver.
pub(crate) mod mock;

/// Serial port driver.
pub mod serial;

/// Relates to config files.
pub mod config;

/// An endpoint- i.e. something which produces output, such as a serial port.
/// But can also be mocked by a file.
pub mod endpoint;

/// Possible errors in this library.
pub mod error;

/// Logging/tracing setup.
pub mod logging;

/// A connected user.
pub mod user;
