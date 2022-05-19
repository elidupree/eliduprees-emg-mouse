use btleplug::api::{
    Central, CentralEvent, CharPropFlags, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::{Manager, Peripheral};
use futures::stream::StreamExt;
use futures::{SinkExt, Stream};
use std::convert::TryInto;
use std::time::Duration;
use tokio::task;
use tokio::time::timeout;

pub fn messages_from_server() -> impl Stream<Item = [u16; 4]> {
    let (mut sender, receiver) = futures::channel::mpsc::unbounded();
    task::spawn(async move {
        let manager = Manager::new().await.unwrap();

        // get the first bluetooth adapter
        let adapters = manager.adapters().await.unwrap();
        let central = adapters.into_iter().nth(0).unwrap();
        let mut events = central.events().await.unwrap();
        central.start_scan(ScanFilter::default()).await.unwrap();

        let server_id;
        let mut server_peripheral;
        loop {
            match events.next().await.unwrap() {
                CentralEvent::DeviceDiscovered(id) => {
                    println!("DeviceDiscovered: {:?}", id);
                    let p = central.peripheral(&id).await.unwrap();
                    if let Ok(Some(pr)) = p.properties().await {
                        if pr.local_name.as_deref() == Some("ELI_EMG_SERVER") {
                            server_id = id;
                            server_peripheral = p;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        async fn connect_and_subscribe(p: &Peripheral) -> anyhow::Result<()> {
            p.connect().await?;
            p.discover_services().await?;
            for characteristic in p.characteristics() {
                if characteristic.properties.contains(CharPropFlags::NOTIFY) {
                    println!("Subscribing to characteristic {:?}", characteristic.uuid);
                    p.subscribe(&characteristic).await?;
                }
            }
            Ok(())
        }

        let mut stream = server_peripheral.notifications().await.unwrap();
        let _ = connect_and_subscribe(&server_peripheral).await;

        loop {
            match timeout(Duration::from_secs(1), stream.next()).await {
                Ok(Some(notification)) => {
                    for chunk in notification.value.chunks_exact(8) {
                        let values: [u16; 4] = chunk
                            .chunks_exact(2)
                            .map(|p| (u16::from(p[0]) << 8) + u16::from(p[1]))
                            .collect::<Vec<u16>>()
                            .try_into()
                            .unwrap();
                        if sender.send(values).await.is_err() {
                            break;
                        }
                    }
                }
                _ => {
                    eprintln!("Timeout...?");
                    if let Ok(p) = central.peripheral(&server_id).await {
                        server_peripheral = p;
                        stream = server_peripheral.notifications().await.unwrap();
                        let _ = connect_and_subscribe(&server_peripheral).await;
                    }
                }
            }
        }
    });

    receiver
}
