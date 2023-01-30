use clap::Parser;
use color_eyre::Result;
use lipsum::lipsum_words;
use serial_keel::client::ClientHandle;
use tracing::{error, info};

/// Mocks a client for testing purposes.
/// Sends/receives messages to/from a server at the given address at the given rates.
/// Messages are random lorem ipsum text.
#[derive(Parser, Debug)]
struct Args {
    /// Server address.
    #[arg(short, long, default_value = "localhost")]
    address: String,

    /// Server port.
    #[arg(short, long, default_value_t = serial_keel::server::DEFAULT_PORT)]
    port: u16,

    /// The name of the mock endpoint.
    #[arg(short, long, default_value = "mock-foo")]
    name: String,

    /// How long to wait before a new message is sent to the mock endpoint.
    /// In milliseconds.
    /// Due to the way mocks work, this will also generate a new receive message.
    #[arg(short, long, default_value = "500")]
    send_interval_ms: usize,

    /// How many lorem ipsum words to send in each message.
    #[arg(short, long, default_value = "10")]
    words: usize,
}

async fn run(args: Args) -> Result<()> {
    let mut client = ClientHandle::new(&args.address, args.port).await?;
    let mock = &mut client.control_mock(&args.name).await?[0];

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(
        args.send_interval_ms as u64,
    ));

    loop {
        let message = lipsum_words(args.words);
        info!(?message, "Sending message");
        mock.write(message.clone()).await?;

        interval.tick().await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    serial_keel::logging::init().await;

    let args = Args::parse();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C, quitting")
        }
        _ = run(args) => {
            error!("Server returned")
        }
    }

    Ok(())
}
