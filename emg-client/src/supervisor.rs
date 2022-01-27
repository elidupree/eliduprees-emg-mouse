use crate::follower::{
    FollowerIntroduction, LocalFollower, RemoteFollower, SupervisedFollower, SupervisedFollowerMut,
};
use crate::webserver::{FrontendState, HistoryFrame, MessageFromFrontend};
use crossbeam::atomic::AtomicCell;
use emg_mouse_shared::ReportFromServer;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, VecDeque};
use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SupervisorOptions {
    pub server_address: String,
    pub gui_port: u16,
    pub follower_port: u16,
}

enum FollowerId {
    Local,
    Remote(String),
}
pub enum MessageToSupervisor {
    FromFrontend(MessageFromFrontend),
    FromServer(ReportFromServer),
    NewFollower(String, SupervisedFollower<RemoteFollower>),
}

pub struct Supervisor {
    start_time: Instant,
    total_inputs: usize,

    local_follower: SupervisedFollower<LocalFollower>,
    remote_followers: HashMap<String, SupervisedFollower<RemoteFollower>>,
    active_follower_id: FollowerId,

    frontend_state_updater: Arc<AtomicCell<Option<FrontendState>>>,

    receiver: Receiver<MessageToSupervisor>,

    enabled: bool,
    mouse_pressed: bool,
    history: VecDeque<HistoryFrame>,
    last_activation: Instant,
}

impl Supervisor {
    fn active_follower(&mut self) -> SupervisedFollowerMut {
        match self.active_follower_id {
            FollowerId::Local => SupervisedFollowerMut::Local(&mut self.local_follower),
            FollowerId::Remote(ref name) => {
                SupervisedFollowerMut::Remote(self.remote_followers.get_mut(name).unwrap())
            }
        }
    }
    fn update_active_follower(&mut self) {
        if self.mouse_pressed {
            return;
        }
        let earliest_remote = self
            .remote_followers
            .iter_mut()
            .map(|(name, follower)| (name, follower.most_recent_mouse_move()))
            .max_by_key(|(_n, t)| t.clone());
        if let Some((name, time)) = earliest_remote {
            if time > self.local_follower.most_recent_mouse_move() {
                self.active_follower_id = FollowerId::Remote(name.clone())
            } else {
                self.active_follower_id = FollowerId::Local
            }
        } else {
            self.active_follower_id = FollowerId::Local
        }
    }
    fn update_frontend(&mut self) {
        let start_time = self.start_time;
        self.frontend_state_updater.store(Some(FrontendState {
            enabled: self.enabled,
            followers: std::iter::once((
                "Local".to_string(),
                (self.local_follower.most_recent_mouse_move() - start_time).as_secs_f64(),
            ))
            .chain(self.remote_followers.iter_mut().map(|(n, f)| {
                (
                    n.clone(),
                    (f.most_recent_mouse_move() - start_time).as_secs_f64(),
                )
            }))
            .collect(),
            history: self.history.clone(),
        }));
    }
    fn handle_message(&mut self, message: MessageToSupervisor) {
        match message {
            MessageToSupervisor::FromFrontend(message) => {
                self.handle_message_from_frontend(message)
            }
            MessageToSupervisor::FromServer(report) => self.handle_report(report),
            MessageToSupervisor::NewFollower(name, follower) => {
                self.remote_followers.insert(name, follower);
            }
        }
    }
    fn handle_message_from_frontend(&mut self, message: MessageFromFrontend) {
        match message {
            MessageFromFrontend::SetEnabled(new_enabled) => {
                if self.mouse_pressed && !new_enabled {
                    self.active_follower().mouse_up();
                    self.mouse_pressed = false;
                }
                self.enabled = new_enabled;
                self.update_frontend()
            }
        }
    }
    fn handle_report(&mut self, report: ReportFromServer) {
        let click_cooldown = Duration::from_millis(400);
        let unclick_cooldown = Duration::from_millis(400);

        self.update_active_follower();

        let value = report.inputs[2] as f64 / 1000.0;
        let time = report.time_since_start.as_secs_f64();
        let recent_values = self
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

        self.history.push_back(HistoryFrame {
            time,
            value,
            click_threshold: recent_max + 0.06,
            too_much_threshold: recent_max + 0.14,
        });
        self.history.retain(|frame| frame.time >= time - 0.8);

        let unclick_possible = value < 0.35;
        if !unclick_possible {
            self.last_activation = Instant::now();
        }
        if self.mouse_pressed {
            if unclick_possible && (Instant::now() - self.last_activation) > unclick_cooldown {
                assert!(self.enabled);
                self.active_follower().mouse_up();
                self.mouse_pressed = false;
            }
        } else {
            let click_possible = self.history.iter().any(|frame| {
                (time - 0.03..time - 0.02).contains(&frame.time)
                    && frame.value > frame.click_threshold
            });
            let too_much = self
                .history
                .iter()
                .any(|frame| frame.time >= time - 0.3 && frame.value > frame.too_much_threshold);
            let move_time = self.active_follower().most_recent_mouse_move();
            let recently_moved = (Instant::now() - move_time) < Duration::from_millis(100);
            if self.enabled
                && click_possible
                && !too_much
                && !recently_moved
                && (Instant::now() - self.last_activation) > click_cooldown
            {
                self.last_activation = Instant::now();
                self.active_follower().mousedown();
                self.mouse_pressed = true;
            }
        }

        self.update_frontend();
        self.total_inputs += 1;
        println!(
            "{}: {}",
            self.total_inputs,
            self.total_inputs as f64 / report.time_since_start.as_secs_f64()
        );
    }

    pub fn new(
        SupervisorOptions {
            server_address,
            gui_port,
            follower_port,
        }: SupervisorOptions,
    ) -> Supervisor {
        let start_time = Instant::now();
        let frontend_state_updater = Arc::new(AtomicCell::new(None));
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn({
            let state_updater = frontend_state_updater.clone();
            let sender = sender.clone();
            move || {
                crate::rocket_glue::launch(
                    state_updater,
                    sender,
                    PathBuf::from("web_frontend"),
                    gui_port,
                );
            }
        });

        let local_follower = SupervisedFollower::new(LocalFollower::new());

        let mut server_stream = BufReader::new(TcpStream::connect(&server_address).unwrap());
        std::thread::spawn({
            let sender = sender.clone();
            move || {
                while let Ok(message) =
                    bincode::deserialize_from::<_, ReportFromServer>(&mut server_stream)
                {
                    sender
                        .send(MessageToSupervisor::FromServer(message))
                        .unwrap();
                }
            }
        });

        std::thread::spawn(move || {
            let listener = TcpListener::bind(("0.0.0.0", follower_port)).unwrap();

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        std::thread::spawn({
                            let sender = sender.clone();
                            move || {
                                let mut read_stream = BufReader::new(stream.try_clone().unwrap());
                                let write_stream = BufWriter::new(stream);
                                if let Ok(introduction) =
                                    bincode::deserialize_from::<_, FollowerIntroduction>(
                                        &mut read_stream,
                                    )
                                {
                                    sender
                                        .send(MessageToSupervisor::NewFollower(
                                            introduction.name,
                                            SupervisedFollower::new(RemoteFollower {
                                                stream: write_stream,
                                                most_recent_mouse_move_updater: Arc::new(
                                                    AtomicCell::new(None),
                                                ),
                                            }),
                                        ))
                                        .unwrap();
                                    while let Ok(message) =
                                        bincode::deserialize_from::<_, ReportFromServer>(
                                            &mut read_stream,
                                        )
                                    {
                                        sender
                                            .send(MessageToSupervisor::FromServer(message))
                                            .unwrap();
                                    }
                                }
                            }
                        });
                    }
                    Err(_e) => { /* connection failed */ }
                }
            }
        });
        Supervisor {
            start_time,
            total_inputs: 0,
            local_follower,
            remote_followers: HashMap::new(),
            active_follower_id: FollowerId::Local,
            frontend_state_updater,
            receiver,
            enabled: false,
            mouse_pressed: false,
            history: VecDeque::new(),
            last_activation: Instant::now(),
        }
    }
    pub fn run(mut self) {
        while let Ok(message) = self.receiver.recv() {
            self.handle_message(message);
        }
    }
}
