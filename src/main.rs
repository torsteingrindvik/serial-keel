use clap::Parser;
use serial_keel::{actions::Response, cli, config::Config, error, logging, server};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() {
    logging::init().await;

    let cli = cli::Cli::parse();

    if let Some(command) = cli.command {
        match command {
            cli::Commands::Examples(example) => match example {
                cli::Examples::Config => {
                    let c = Config::example();
                    println!("{}", c.serialize_pretty());
                    return;
                }
                cli::Examples::ControlGranted => {
                    let example: Result<_, error::Error> = Ok(Response::example_control_granted());
                    let serialized = serde_json::to_string_pretty(&example).unwrap();
                    println!("{serialized}");
                    return;
                }
            },
        }
    }

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
            error!("Server returned")
        }
    }

    logging::shutdown();
}
