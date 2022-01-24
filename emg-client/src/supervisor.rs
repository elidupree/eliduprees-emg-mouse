use crate::follower::Follower;
use crate::webserver::{FrontendState, MessageFromFrontend};
use crossbeam::atomic::AtomicCell;
use emg_mouse_shared::ReportFromServer;
use std::collections::VecDeque;
use std::io::BufReader;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SupervisorOptions {
    pub server_address: String,
    pub gui_port: u16,
}

pub fn run(
    SupervisorOptions {
        server_address,
        gui_port,
    }: SupervisorOptions,
) {
    let state_updater = Arc::new(AtomicCell::new(None));
    let mut frontend_state = FrontendState {
        enabled: false,
        history: VecDeque::new(),
    };
    let (sender_from_frontend, receiver_from_frontend) = mpsc::sync_channel(1);
    std::thread::spawn({
        let state_updater = state_updater.clone();
        move || {
            crate::rocket_glue::launch(
                state_updater,
                sender_from_frontend,
                PathBuf::from("web_frontend"),
                gui_port,
            );
        }
    });

    let mut local_follower = Follower::new();

    let mut server_stream = BufReader::new(TcpStream::connect(&server_address).unwrap());

    let mut mouse_pressed = false;
    let click_threshold = 450;
    let click_cooldown = Duration::from_millis(200);
    let unclick_threshold = 350;
    let mut enabled = false;

    let start = Instant::now();
    let mut total_inputs = 0;
    let mut last_activation = Instant::now();

    while let Ok(report) = bincode::deserialize_from::<_, ReportFromServer>(&mut server_stream) {
        println!("{:?}", report);
        while let Ok(message) = receiver_from_frontend.try_recv() {
            match message {
                MessageFromFrontend::SetEnabled(new_enabled) => {
                    if mouse_pressed && !new_enabled {
                        local_follower.mouse_up();
                    }
                    enabled = new_enabled;
                }
            }
        }
        let left_button = report.inputs[2];
        if left_button >= unclick_threshold {
            last_activation = Instant::now();
        }
        if mouse_pressed {
            if left_button < unclick_threshold
                && (Instant::now() - last_activation) > click_cooldown
            {
                assert!(enabled);
                local_follower.mouse_up();
                mouse_pressed = false;
            }
        } else {
            if left_button > click_threshold {
                if enabled {
                    local_follower.mousedown();
                    mouse_pressed = true;
                }
            }
        }
        frontend_state.enabled = enabled;
        frontend_state
            .history
            .push_back(left_button as f64 / 3300.0);
        if frontend_state.history.len() > 700 {
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

    if mouse_pressed {
        local_follower.mouse_up();
    }
}
