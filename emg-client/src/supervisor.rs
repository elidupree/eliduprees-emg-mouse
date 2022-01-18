use emg_mouse_shared::Report;
use enigo::{Enigo, MouseButton, MouseControllable};
use rodio::source::Buffered;
use rodio::{Decoder, OutputStream, Source};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;

pub struct SupervisorOptions {
    pub server_address: String,
}

fn load_sound(path: impl AsRef<Path>) -> Buffered<impl Source<Item = f32>> {
    Decoder::new(BufReader::new(File::open(path).unwrap()))
        .unwrap()
        .convert_samples()
        .buffered()
}

pub fn run(SupervisorOptions { server_address }: SupervisorOptions) {
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let mut enigo = Enigo::new();

    let click_sound = load_sound("../media/click.wav");
    let unclick_sound = load_sound("../media/unclick.wav");

    let server_stream = BufReader::new(TcpStream::connect(&server_address).unwrap());

    let mut mouse_pressed = false;
    let click_threshold = 500;
    let unclick_threshold = 200;
    let do_clicks = false;

    for line in server_stream.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        let data: Report = serde_json::from_str(&line).unwrap();
        println!("{:?}", data);
        if mouse_pressed {
            if data.left_button < unclick_threshold {
                if do_clicks {
                    enigo.mouse_down(MouseButton::Left);
                }
                stream_handle.play_raw(click_sound.clone()).unwrap();
                mouse_pressed = false;
            }
        } else {
            if data.left_button > click_threshold {
                if do_clicks {
                    enigo.mouse_up(MouseButton::Left);
                }
                stream_handle.play_raw(unclick_sound.clone()).unwrap();
                mouse_pressed = true;
            }
        }
    }

    if mouse_pressed && do_clicks {
        enigo.mouse_down(MouseButton::Left);
    }
}
