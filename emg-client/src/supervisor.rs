use crate::follower::{Follower, LocalFollower};
use crate::webserver::{FrontendState, HistoryFrame, MessageFromFrontend};
use crossbeam::atomic::AtomicCell;
use emg_mouse_shared::ReportFromServer;
use ordered_float::OrderedFloat;
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

    let mut local_follower = LocalFollower::new();

    let mut server_stream = BufReader::new(TcpStream::connect(&server_address).unwrap());

    let mut mouse_pressed = false;
    let click_cooldown = Duration::from_millis(400);
    let unclick_cooldown = Duration::from_millis(400);
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
                        mouse_pressed = false;
                    }
                    enabled = new_enabled;
                }
            }
        }
        let value = report.inputs[2] as f64 / 1000.0;
        let time = report.time_since_start.as_secs_f64();
        let recent_values = frontend_state
            .history
            .iter()
            .filter(|frame| {
                (time - 0.3..time - 0.1).contains(&frame.time) &&
                // when we've analyzed something as a spike, also do not count it among the noise
                frame.value < frame.click_threshold
            })
            .map(|frame| frame.value);
        let recent_max = recent_values
            .max_by_key(|&v| OrderedFloat(v))
            .unwrap_or(1.0);

        frontend_state.enabled = enabled;
        frontend_state.history.push_back(HistoryFrame {
            time,
            value,
            click_threshold: recent_max + 0.06,
            too_much_threshold: recent_max + 0.14,
        });
        frontend_state
            .history
            .retain(|frame| frame.time >= time - 0.8);

        let unclick_possible = value < 0.35;
        if !unclick_possible {
            last_activation = Instant::now();
        }
        if mouse_pressed {
            if unclick_possible && (Instant::now() - last_activation) > unclick_cooldown {
                assert!(enabled);
                local_follower.mouse_up();
                mouse_pressed = false;
            }
        } else {
            let click_possible = frontend_state.history.iter().any(|frame| {
                (time - 0.03..time - 0.02).contains(&frame.time)
                    && frame.value > frame.click_threshold
            });
            let too_much = frontend_state
                .history
                .iter()
                .any(|frame| frame.time >= time - 0.3 && frame.value > frame.too_much_threshold);
            if enabled
                && click_possible
                && !too_much
                && (Instant::now() - last_activation) > click_cooldown
            {
                last_activation = Instant::now();
                local_follower.mousedown();
                mouse_pressed = true;
            }
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
