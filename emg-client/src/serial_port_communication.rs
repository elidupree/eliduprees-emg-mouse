use arrayvec::ArrayVec;
use futures::channel::mpsc::UnboundedSender;
use futures::{SinkExt, Stream};
use tokio::io::AsyncReadExt;
use tokio::task;
use tokio_serial::SerialPortBuilderExt;

#[derive(Debug)]
pub struct ReportFromServer {
    pub server_run_id: u64,
    pub first_sample_index: u64,
    pub samples: Vec<[u16; 4]>,
}

const MAX_SEND_SIZE: usize = 16 + 82 * 6;

async fn read_messages(mut sender: UnboundedSender<ReportFromServer>) -> Result<!, anyhow::Error> {
    let mut stream = tokio_serial::new("/dev/ttyUSB0", 115200)
        .open_native_async()
        .unwrap();
    let mut buffer = [0; MAX_SEND_SIZE];
    let mut recent = ArrayVec::<_, 8>::new();
    loop {
        let val = stream.read_u8().await?;
        if recent.len() >= 8 {
            recent.remove(0);
        }
        recent.push(val);
        if &recent == "emg_data".as_bytes() {
            recent.clear();
            let server_run_id = stream.read_u64_le().await?;
            let first_sample_index = stream.read_u64_le().await?;
            let num_samples = stream.read_u16_le().await?;
            let sample_data = &mut buffer[..(num_samples as usize) * 6];
            stream.read_exact(sample_data).await?;

            let samples = sample_data
                .chunks_exact(6)
                .map(|chunk| {
                    [
                        (u16::from(chunk[0]) << 4) + (u16::from(chunk[4]) >> 4),
                        (u16::from(chunk[1]) << 4) + (u16::from(chunk[4]) & 15),
                        (u16::from(chunk[2]) << 4) + (u16::from(chunk[5]) >> 4),
                        (u16::from(chunk[3]) << 4) + (u16::from(chunk[5]) & 15),
                    ]
                })
                .collect::<Vec<_>>();
            let report = ReportFromServer {
                server_run_id,
                first_sample_index,
                samples,
            };

            sender.send(report).await?;
        }
    }
}

pub fn messages_from_server() -> impl Stream<Item = ReportFromServer> {
    let (sender, receiver) = futures::channel::mpsc::unbounded();
    task::spawn(async move {
        match read_messages(sender).await {
            Ok(n) => match n {},
            Err(e) => println!("Server receive thread closed due to error: {}", e),
        }
    });

    receiver
}
