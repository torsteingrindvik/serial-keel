use color_eyre::Result;
use serial_keel::client::ClientHandle;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    serial_keel::logging::init().await;

    let mut client = ClientHandle::new("127.0.0.1", 3123).await?;

    let mut writer = client.control_mock("/dev/ttyACM0").await?;
    let mut observer = client.observe_mock("/dev/ttyACM0").await?;

    writer.write("Hello\nWorld\nBye!").await?;

    for _ in 0..3 {
        let msg = observer.next_message().await;
        info!("{msg}");
    }

    Ok(())
}
