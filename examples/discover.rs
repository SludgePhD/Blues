use blues::{device::Device, Adapter, Session};

#[pollster::main]
async fn main() -> blues::Result<()> {
    env_logger::builder()
        .filter_module(env!("CARGO_PKG_NAME"), log::LevelFilter::Debug)
        .init();

    let session = Session::new().await?;
    let adapter = Adapter::open(&session).await?;
    println!(
        "adapter address: {} ({:?})",
        adapter.address().await?,
        adapter.address_type().await?,
    );

    adapter.start_discovery().await?;
    println!("device discovery started...");

    let mut devices = adapter.device_stream().await?;
    loop {
        let device = devices.next().await?;

        if let Err(e) = print_device(&device).await {
            eprintln!("error querying device: {}", e);
        }

        if !adapter.is_discovering().await? {
            eprintln!("discovery stopped externally, exiting");
            return Ok(());
        }
    }
}

async fn print_device(device: &Device) -> blues::Result<()> {
    println!(
        "saw {} ({:?}): {}",
        device.address().await?,
        device.address_type().await?,
        device.alias().await?,
    );
    println!("services: {:?}", device.service_uuids().await?);

    Ok(())
}
