use tokio::io::AsyncReadExt;
use tokio_serial::SerialPortBuilderExt;

const MAX_SEND_SIZE: usize = 16 + 82 * 6;

#[tokio::main]
async fn main() {
    let mut stream = tokio_serial::new("/dev/ttyUSB0", 115200)
        .open_native_async()
        .unwrap();
    let mut buffer = [0; MAX_SEND_SIZE];
    let mut i = 0;
    while let Ok(bytes_read) = dbg!(stream.read(&mut buffer).await) {
        let message = &buffer[..bytes_read];
        if let Ok(message) = std::str::from_utf8(message) {
            dbg!(message);
        }
        if let Some(message_start) = message
            .windows(8)
            .position(|window| window == "emg_data".as_bytes())
        {
            let data = &message[message_start + 8..];
            let server_run_id = u64::from_le_bytes((&data[0..8]).try_into().unwrap());
            let first_sample_index = u64::from_le_bytes((&data[8..16]).try_into().unwrap());
            let num_samples = u16::from_le_bytes((&data[16..18]).try_into().unwrap());
            let samples = data[18..]
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
            dbg!((server_run_id, first_sample_index, num_samples, samples));
        }
        // if message.len() >= 8 && &message[..8] == "emg_data".as_bytes() {
        //     dbg!(message);
        // }
        //dbg!(message);
        i += 1;
        if i > 100 {
            break;
        }
    }
    println!("Hello, world!");
}
