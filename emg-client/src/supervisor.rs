use crate::webserver::FrontendState;
use crossbeam::atomic::AtomicCell;
use emg_mouse_shared::ReportFromServer;
use enigo::{Enigo, MouseButton, MouseControllable};
use rodio::source::Buffered;
use rodio::{Decoder, OutputStream, Source};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SupervisorOptions {
    pub server_address: String,
    pub gui_port: u16,
}

fn load_sound(path: impl AsRef<Path>) -> Buffered<impl Source<Item = f32>> {
    Decoder::new(BufReader::new(File::open(path).unwrap()))
        .unwrap()
        .convert_samples()
        .buffered()
}

pub fn run(
    SupervisorOptions {
        server_address,
        gui_port,
    }: SupervisorOptions,
) {
    let state_updater = Arc::new(AtomicCell::new(None));
    let mut frontend_state = FrontendState {
        history: VecDeque::new(),
    };
    let (sender, _receiver) = mpsc::sync_channel(1);
    std::thread::spawn({
        let state_updater = state_updater.clone();
        move || {
            crate::rocket_glue::launch(
                state_updater,
                sender,
                PathBuf::from("web_frontend"),
                gui_port,
            );
        }
    });

    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let mut enigo = Enigo::new();

    let click_sound = load_sound("../media/click.wav");
    let unclick_sound = load_sound("../media/unclick.wav");

    let mut server_stream = BufReader::new(TcpStream::connect(&server_address).unwrap());

    let mut mouse_pressed = false;
    let click_threshold = 450;
    let click_cooldown = Duration::from_millis(200);
    let unclick_threshold = 350;
    let do_clicks = true;

    let start = Instant::now();
    let mut total_inputs = 0;
    let mut last_activation = Instant::now();

    while let Ok(report) = bincode::deserialize_from::<_, ReportFromServer>(&mut server_stream) {
        println!("{:?}", report);
        let left_button = report.inputs[2];
        if left_button >= unclick_threshold {
            last_activation = Instant::now();
        }
        if mouse_pressed {
            if left_button < unclick_threshold
                && (Instant::now() - last_activation) > click_cooldown
            {
                if do_clicks {
                    enigo.mouse_up(MouseButton::Left);
                }
                stream_handle.play_raw(unclick_sound.clone()).unwrap();
                mouse_pressed = false;
            }
        } else {
            if left_button > click_threshold {
                if do_clicks {
                    enigo.mouse_down(MouseButton::Left);
                }
                stream_handle.play_raw(click_sound.clone()).unwrap();
                mouse_pressed = true;
            }
        }
        frontend_state
            .history
            .push_back(left_button as f64 / 3300.0);
        if frontend_state.history.len() > 2500 {
            frontend_state.history.pop_front();
        }
        state_updater.store(Some(frontend_state.clone()));
        let now = Instant::now();
        total_inputs += 1;
        println!(
            "{}: {}",
            total_inputs,
            total_inputs as f64 / (now - start).as_secs_f64()
        );
    }

    if mouse_pressed && do_clicks {
        enigo.mouse_up(MouseButton::Left);
    }
}
