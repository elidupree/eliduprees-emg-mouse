use tokio::io::AsyncReadExt;
use tokio_serial::SerialPortBuilderExt;

const MAX_SEND_SIZE: usize = 16 + 82 * 6;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = tokio_serial::new("/dev/ttyUSB0", 115200)
        .open_native_async()
        .unwrap();
    let mut buffer = [0; MAX_SEND_SIZE];
    let mut recent = Vec::with_capacity(8);
    loop {
        let val = stream.read_u8().await?;
        if recent.len() >= 8 {
            recent.pop();
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
            dbg!((server_run_id, first_sample_index, num_samples, samples,));
        }
    }
}
