use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// The command line interface for serial keel.
#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    /// Path to a configuration file
    pub config: Option<PathBuf>,

    /// Subcommands
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Commands available in the command line interface.
#[derive(Subcommand)]
pub enum Commands {
    /// Examples for user convenience.
    #[clap(subcommand)]
    Examples(Examples),
}

/// Helpful examples for users.
#[derive(Subcommand, Clone)]
pub enum Examples {
    /// Show an example of a configuration file's contents.
    Config,

    /// Show an example JSON response to granted control.
    ControlGranted,

    /// Show an example JSON request of writing a message to an endpoint.
    WriteMessage,

    /// Show an example JSON request of controlling any endpoint matching the provided label(s).
    ControlAny,

    /// Show an example JSON request of writing bytes to an endpoint.
    WriteMessageBytes,

    /// Show an example JSON response (from server to user) of a new message.
    NewMessage,
}
