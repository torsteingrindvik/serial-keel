use clap::Parser;
use serial_keel::{
    actions::{Action, Response},
    cli,
    config::Config,
    error, logging, server,
};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    logging::init().await;

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
                cli::Examples::WriteMessage => {
                    let example = Action::example_write();
                    let serialized = serde_json::to_string_pretty(&example).unwrap();
                    println!("{serialized}");
                    return;
                }
                cli::Examples::WriteMessageBytes => {
                    let example = Action::example_write_bytes();
                    let serialized = serde_json::to_string_pretty(&example).unwrap();
                    println!("{serialized}");
                    return;
                }
                cli::Examples::NewMessage => {
                    let example: Result<_, error::Error> = Ok(Response::example_new_message());
                    let serialized = serde_json::to_string_pretty(&example).unwrap();
                    println!("{serialized}");
                    return;
                }
                cli::Examples::ControlAny => {
                    let example = Action::example_control_any();
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
