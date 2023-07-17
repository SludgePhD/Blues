use std::process;

use blues::{uuid::Uuid, Adapter, Session};

// https://www.bluetooth.com/specifications/assigned-numbers/
const HEART_RATE_SERVICE: Uuid = Uuid::from_u16(0x180D);
const HEART_RATE_MEASUREMENT_CHARACTERISTIC: Uuid = Uuid::from_u16(0x2A37);

#[pollster::main]
async fn main() -> blues::Result<()> {
    env_logger::builder()
        .filter_module(env!("CARGO_PKG_NAME"), log::LevelFilter::Debug)
        .filter_module(env!("CARGO_CRATE_NAME"), log::LevelFilter::Debug)
        .init();

    let session = Session::new().await?;
    let adapter = Adapter::open(&session).await?;
    log::debug!(
        "adapter address: {} ({:?})",
        adapter.address().await?,
        adapter.address_type().await?,
    );

    adapter.start_discovery().await?;

    log::info!("device discovery started...");
    let mut devices = adapter.device_stream().await?;
    let device = loop {
        let device = devices.next().await?;

        log::info!(
            "device {} ({:?}): {}",
            device.address().await?,
            device.address_type().await?,
            device.alias().await?,
        );
        let services = device.service_uuids().await?;
        log::info!("services: {:?}", services);

        if services.contains(&HEART_RATE_SERVICE) {
            break device.clone();
        }
    };

    adapter.stop_discovery().await?;

    log::info!("connecting to {}", device.alias().await?);
    device.connect().await?;

    log::debug!("enumerating services");
    let services = device.gatt_services().await?;
    let mut characteristic = None;
    for service in services {
        if service.uuid().await? == HEART_RATE_SERVICE {
            let ch = service
                .characteristic(HEART_RATE_MEASUREMENT_CHARACTERISTIC)
                .await?;
            let flags = ch.flags().await?;
            log::debug!("characteristic flags: {:?}", flags);
            characteristic = Some(ch);
        }
    }

    let Some(characteristic) = characteristic else {
        eprintln!("error: couldn't find heart rate measurement characteristic");
        process::exit(1);
    };

    let mut stream = characteristic.subscribe().await?;
    loop {
        let data = stream.next().await?;
        let Some(meas) = parse_characteristic(&data) else {
            eprintln!("couldn't parse measurement: {:02x?}", data);
            process::exit(1);
        };

        println!("{} BPM", meas.rate);
    }
}

struct HeartRateMeasurement {
    rate: u16,
}

fn parse_characteristic(value: &[u8]) -> Option<HeartRateMeasurement> {
    let flags = *value.get(0)?;
    let flags = Flags::from_bits_retain(flags);

    let rate = if flags.contains(Flags::RATE_U16) {
        let mut b = [0; 2];
        b.copy_from_slice(value.get(1..3)?);
        u16::from_le_bytes(b)
    } else {
        u16::from(*value.get(1)?)
    };
    // Ignore everything else, we don't really care.

    Some(HeartRateMeasurement { rate })
}

bitflags::bitflags! {
    struct Flags: u8 {
        const RATE_U16 = 1 << 0;
        const SENSOR_CONTACT_STATUS = 1 << 1;
        const SENSOR_CONTACT_SUPPORT = 1 << 2;
        const ENERGY_EXPENDED = 1 << 3;
        const RR_INTERVAL = 1 << 4;
    }
}
