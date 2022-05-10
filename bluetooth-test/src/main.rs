use btleplug::api::{
    bleuuid::uuid_from_u16, bleuuid::BleUuid, Central, CentralEvent, CharPropFlags, Manager as _,
    Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::stream::StreamExt;
use std::time::{Duration, Instant};

//const LIGHT_CHARACTERISTIC_UUID: Uuid = uuid_from_u16(0xFFE9);

#[tokio::main]
async fn main() {
    let manager = Manager::new().await.unwrap();

    // get the first bluetooth adapter
    let adapters = manager.adapters().await.unwrap();
    let central = adapters.into_iter().nth(0).unwrap();

    let mut events = central.events().await.unwrap();
    // start scanning for devices
    central.start_scan(ScanFilter::default()).await.unwrap();
    // instead of waiting, you can use central.events() to get a stream which will
    // notify you of new devices, for an example of that see examples/event_driven_discovery.rs
    //tokio::time::sleep(Duration::from_secs(2)).await;

    while let Some(event) = events.next().await {
        match event {
            CentralEvent::DeviceDiscovered(id) => {
                println!("DeviceDiscovered: {:?}", id);
                let p = central.peripheral(&id).await.unwrap();
                dbg!(&p);
                if let Ok(Some(pr)) = p.properties().await {
                    dbg!(&pr.local_name);
                    if pr.local_name.as_deref() == Some("ESP_GATTS_DEMO") {
                        if p.connect().await.is_ok() {
                            p.discover_services().await.unwrap();
                            dbg!(p.characteristics());
                            let mut stream = p.notifications().await.unwrap();
                            for characteristic in p.characteristics() {
                                if characteristic.properties.contains(CharPropFlags::NOTIFY) {
                                    println!(
                                        "Subscribing to characteristic {:?}",
                                        characteristic.uuid
                                    );
                                    p.subscribe(&characteristic).await.unwrap();
                                }
                            }
                            let start = Instant::now();
                            let mut count = 0;
                            let mut amount = 0;
                            while let Some(notification) = stream.next().await {
                                // dbg!(notification);
                                count += 1;
                                amount += notification.value.len();
                                if count % 1 == 0 {
                                    let e = start.elapsed();
                                    println!(
                                        "{}, {}: {}, {:?}, {}, {}",
                                        count,
                                        amount,
                                        notification.value.len(),
                                        e,
                                        amount as f64 / e.as_secs_f64(),
                                        count as f64 / e.as_secs_f64()
                                    );
                                }
                            }
                        }
                    }
                }
            }
            CentralEvent::DeviceConnected(id) => {
                println!("DeviceConnected: {:?}", id);
            }
            CentralEvent::DeviceDisconnected(id) => {
                println!("DeviceDisconnected: {:?}", id);
            }
            CentralEvent::ManufacturerDataAdvertisement {
                id,
                manufacturer_data,
            } => {
                println!(
                    "ManufacturerDataAdvertisement: {:?}, {:?}",
                    id, manufacturer_data
                );
            }
            CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                println!("ServiceDataAdvertisement: {:?}, {:?}", id, service_data);
            }
            CentralEvent::ServicesAdvertisement { id, services } => {
                let services: Vec<String> =
                    services.into_iter().map(|s| s.to_short_string()).collect();
                println!("ServicesAdvertisement: {:?}, {:?}", id, services);
            }
            _ => {}
        }
    }
    for p in central.peripherals().await.unwrap() {
        dbg!(&p);
        // if p.properties()
        //     .await
        //     .unwrap()
        //     .unwrap()
        //     .local_name
        //     .iter()
        //     .any(|name| name.contains("LEDBlue"))
        // {
        //     return Some(p);
        // }
        if p.connect().await.is_ok() {
            p.discover_services().await.unwrap();
            dbg!(p.characteristics());
        }
    }

    // find the device we're interested in
    // let light = find_light(&central).await.unwrap();
    //
    // // connect to the device
    // light.connect().await?;
    //
    // // discover services and characteristics
    // light.discover_services().await?;
    //
    // // find the characteristic we want
    // let chars = light.characteristics();
    // let cmd_char = chars.iter().find(|c| c.uuid == LIGHT_CHARACTERISTIC_UUID).unwrap();
    //
    // // dance party
    // let mut rng = thread_rng();
    // for _ in 0..20 {
    //     let color_cmd = vec![0x56, rng.gen(), rng.gen(), rng.gen(), 0x00, 0xF0, 0xAA];
    //     light.write(&cmd_char, &color_cmd, WriteType::WithoutResponse).await?;
    //     time::sleep(Duration::from_millis(200)).await;
    // }
}
