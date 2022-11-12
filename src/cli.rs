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
    /// Show an example of a configuration file's contents.
    ConfigExample,
}
