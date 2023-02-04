use clap::Parser;
use color_eyre::Result;
use serial_keel::{cli, config::Config, logging, server};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    if let Some(command) = cli.command {
        cli::handle_command(command);

        return Ok(());
    }

    logging::init().await;

    let config = if let Some(config_path) = cli.config {
        debug!(?config_path, "Config from path");
        Config::new_from_path(config_path)
    } else {
        debug!("Default config");
        Config::default()
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C, quitting")
        }
        _ = server::run_on_port(config, 3123) => {
            error!("Server returned");
            return Err(color_eyre::eyre::eyre!("Server stopped unexpectedly"));
        }
    }

    logging::shutdown();

    Ok(())
}
