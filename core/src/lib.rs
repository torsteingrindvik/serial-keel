#![deny(missing_docs)]
#![doc = include_str!("../../README.md")]

/// The actions that can be sent from a connecting user,
/// as well as the responses.
pub mod actions;

/// Code relating to setting up the server which sets up connections and spawns handlers for clients.
pub mod server;

/// Clients.
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

/// Events the server emits.
pub mod events;
