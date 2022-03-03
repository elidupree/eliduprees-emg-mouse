use crate::follower::{
    FollowerIntroduction, LocalFollower, MessageFromFollower, RemoteFollower, SupervisedFollower,
    SupervisedFollowerMut,
};
use crate::remote_time_estimator::RemoteTimeEstimator;
use crate::signal::Signal;
use crate::supervisor::MessageToSupervisor::PokeServers;
use crate::utils::{DatagramsExt, IncomingUniStreamsExt};
use crate::webserver::{FrontendState, MessageFromFrontend};
use anyhow::{bail, Context};
use crossbeam::atomic::AtomicCell;
use emg_mouse_shared::{
    MessageToServer, OwnedSamplesArray, ReportFromServer, Samples, ServerRunId,
};
use log::info;
use rodio::OutputStream;
use rustfft::FftPlanner;
use statrs::statistics::Statistics;
use std::collections::HashMap;
use std::convert::TryInto;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task;
use tokio_stream::StreamExt;

pub struct SupervisorOptions {
    pub server_address: String,
    pub gui_port: u16,
    pub follower_port: u16,
}

enum FollowerId {
    Local,
    Remote(String),
}
#[derive(Debug)]
pub enum MessageToSupervisor {
    FromFrontend(MessageFromFrontend),
    FromServer {
        server_index: usize,
        report: ReportFromServer<OwnedSamplesArray>,
    },
    NewFollower(String, SupervisedFollower<RemoteFollower>),
    FromFollower(String, MessageFromFollower),
    PokeServers,
}

pub struct SupervisedServer {
    address: SocketAddr,
    latest_run_id: Option<ServerRunId>,
    old_run_ids: Vec<ServerRunId>,
    latest_received_sample_index: u64,
    remote_time_estimator: RemoteTimeEstimator,
    signals: [Signal; 4],
}

pub struct Supervisor {
    start_time: Instant,
    total_inputs: usize,

    local_follower: SupervisedFollower<LocalFollower>,
    remote_followers: HashMap<String, SupervisedFollower<RemoteFollower>>,
    active_follower_id: FollowerId,

    servers: Vec<SupervisedServer>,
    server_socket: Arc<UdpSocket>,

    frontend_state_updater: Arc<AtomicCell<Option<FrontendState>>>,

    receiver: mpsc::Receiver<MessageToSupervisor>,

    enabled: bool,
    mouse_pressed: bool,
    inputs_since_scroll_start: usize,

    fft_planner: FftPlanner<f64>,
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
        let latest_time = self.servers[0].signals[0]
            .history
            .back()
            .map_or(0.0, |f| f.time);
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
            histories: self.servers[0]
                .signals
                .iter()
                .map(|s| {
                    s.history
                        .iter()
                        .filter(|f| f.time >= latest_time - 0.8)
                        .cloned()
                        .collect()
                })
                .collect(),
            frequencies_histories: self.servers[0]
                .signals
                .iter()
                .map(|s| s.frequencies_history.clone())
                .collect(),
        }));
    }
    fn handle_message(&mut self, message: MessageToSupervisor) {
        match message {
            MessageToSupervisor::FromFrontend(message) => {
                self.handle_message_from_frontend(message)
            }
            MessageToSupervisor::FromServer {
                server_index,
                report,
            } => self.handle_report(server_index, report),
            MessageToSupervisor::NewFollower(name, follower) => {
                self.remote_followers.insert(name, follower);
            }
            MessageToSupervisor::FromFollower(name, message) => {
                self.handle_message_from_follower(name, message)
            }
            PokeServers => {
                self.poke_servers();
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
    fn handle_message_from_follower(&mut self, name: String, message: MessageFromFollower) {
        match message {
            MessageFromFollower::MouseMoved { time_since_start } => {
                let now = Instant::now();
                let follower = self.remote_followers.get_mut(&name).unwrap();
                follower.observe_message(time_since_start, now);
                follower.remote_mouse_moved(time_since_start);
            }
        }
    }
    fn poke_server(&self, server_index: usize) {
        let server = &self.servers[server_index];
        let server_socket = self.server_socket.clone();
        let address = server.address;
        let buf = bincode::serialize(&MessageToServer {
            server_run_id: server.latest_run_id.unwrap_or(0),
            latest_received_sample_index: server.latest_received_sample_index,
        })
        .unwrap();
        task::spawn(async move {
            let _ = server_socket.send_to(&buf, address).await;
        });
    }
    fn poke_servers(&self) {
        for (i, _) in self.servers.iter().enumerate() {
            self.poke_server(i);
        }
    }
    fn handle_report(&mut self, server_index: usize, report: ReportFromServer<OwnedSamplesArray>) {
        let server = &mut self.servers[server_index];
        if server.old_run_ids.contains(&report.server_run_id) {
            return;
        }
        if server.latest_run_id != Some(report.server_run_id) {
            if let Some(current) = server.latest_run_id {
                server.old_run_ids.push(current);
            }
            server.latest_run_id = Some(report.server_run_id);
            server.remote_time_estimator = RemoteTimeEstimator::new(Duration::from_micros(50));
            server.latest_received_sample_index = 0;
            server.signals = [Signal::new(), Signal::new(), Signal::new(), Signal::new()];
        }

        let num_new_samples = report
            .latest_sample_index
            .saturating_sub(server.latest_received_sample_index);

        if num_new_samples > 0 {
            server.latest_received_sample_index = report.latest_sample_index;

            self.poke_server(server_index);

            let new_samples = &report.samples[report
                .samples
                .len()
                .saturating_sub(num_new_samples.try_into().unwrap())..];
            for samples in new_samples {
                self.handle_samples(server_index, samples);
            }
            self.update_frontend();
        }
    }
    fn handle_samples(&mut self, server_index: usize, samples: &Samples) {
        self.local_follower.update_most_recent_mouse_move();
        self.update_active_follower();

        let _average = samples.inputs.iter().map(|&i| i as f64).mean();

        let mouse_active_before = self.servers[server_index].signals[2].is_active();
        for (signal, &input) in self.servers[server_index]
            .signals
            .iter_mut()
            .zip(&samples.inputs)
        {
            signal.receive_raw(
                input as f64, /*- average*/
                samples.time_since_start,
                &mut self.fft_planner,
            )
        }

        if self.servers[server_index].signals[2].is_active() != mouse_active_before {
            if self.servers[server_index].signals[2].is_active() {
                let move_time = self.active_follower().most_recent_mouse_move();
                let recently_moved = (Instant::now() - move_time) < Duration::from_millis(50);
                let anywhere_near_recently_moved =
                    (Instant::now() - move_time) < Duration::from_millis(10000);
                if self.enabled && !recently_moved && anywhere_near_recently_moved {
                    self.active_follower().mousedown();
                    self.mouse_pressed = true;
                }
            } else {
                if self.mouse_pressed {
                    assert!(self.enabled);
                    self.active_follower().mouse_up();
                    self.mouse_pressed = false;
                }
            }
        }

        if self.enabled
            && self.servers[server_index].signals[0].is_active()
                != self.servers[server_index].signals[1].is_active()
        {
            fn progress(inputs: usize) -> usize {
                let s = 400;
                (inputs * s + inputs * inputs) / (300 * s)
            }
            if progress(self.inputs_since_scroll_start + 1)
                > progress(self.inputs_since_scroll_start)
            {
                if self.servers[server_index].signals[0].is_active() {
                    self.active_follower().scroll_y(1);
                } else {
                    self.active_follower().scroll_y(-1);
                }
            }
            self.inputs_since_scroll_start += 1;
        } else {
            self.inputs_since_scroll_start = 0;
        }

        self.total_inputs += 1;
        println!(
            "{}: {:.2}, {}",
            self.total_inputs,
            self.total_inputs as f64 / samples.time_since_start.as_secs_f64(),
            samples.time_since_start.as_micros(),
        );
    }

    pub async fn run(
        SupervisorOptions {
            server_address,
            gui_port,
            follower_port,
        }: SupervisorOptions,
    ) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let frontend_state_updater = Arc::new(AtomicCell::new(None));
        let (sender, receiver) = mpsc::channel(2);

        let server_socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await?);
        let server_addresses = [server_address.parse::<SocketAddr>().unwrap()];
        task::spawn({
            let sender = sender.clone();
            let server_socket = server_socket.clone();
            async move {
                let mut buf = vec![0u8; 65536];
                while let Ok((size, address)) = server_socket.recv_from(&mut buf).await {
                    if let Some(server_index) = server_addresses.iter().position(|&a| a == address)
                    {
                        if let Ok(report) = bincode::deserialize(&buf[..size]) {
                            sender
                                .send(MessageToSupervisor::FromServer {
                                    server_index,
                                    report,
                                })
                                .await
                                .unwrap();
                        }
                    }
                }
            }
        });

        task::spawn({
            let sender = sender.clone();
            async move {
                let path = Path::new("secrets");
                let cert_path = path.join("local_cert.der");
                let key_path = path.join("local_private_key.der");
                let (cert, key) = match std::fs::read(&cert_path)
                    .and_then(|x| Ok((x, std::fs::read(&key_path)?)))
                {
                    Ok(x) => x,
                    Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                        info!("generating self-signed certificate");
                        let cert =
                            rcgen::generate_simple_self_signed(vec!["EMG_supervisor".into()])
                                .unwrap();
                        let key = cert.serialize_private_key_der();
                        let cert = cert.serialize_der().unwrap();
                        std::fs::create_dir_all(&path)
                            .context("failed to create certificate directory")?;
                        std::fs::write(&cert_path, &cert).context("failed to write certificate")?;
                        std::fs::write(&key_path, &key).context("failed to write private key")?;
                        (cert, key)
                    }
                    Err(e) => {
                        bail!("failed to read certificate: {}", e);
                    }
                };

                let key = rustls::PrivateKey(key);
                let cert = rustls::Certificate(cert);
                let server_crypto = rustls::ServerConfig::builder()
                    .with_safe_defaults()
                    .with_no_client_auth()
                    .with_single_cert(vec![cert], key)?;
                let server_config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));

                let (_endpoint, mut incoming) =
                    quinn::Endpoint::server(server_config, ([0, 0, 0, 0], follower_port).into())?;

                while let Some(connection) = incoming.next().await {
                    let sender = sender.clone();
                    task::spawn(async move {
                        match connection.await {
                            Ok(quinn::NewConnection {
                                connection,
                                mut uni_streams,
                                mut datagrams,
                                ..
                            }) => {
                                if let Ok(Some(introduction)) = uni_streams
                                    .next_bincode_oneshot::<FollowerIntroduction>()
                                    .await
                                {
                                    sender
                                        .send(MessageToSupervisor::NewFollower(
                                            introduction.name.clone(),
                                            SupervisedFollower::new(RemoteFollower::new(
                                                connection,
                                            )),
                                        ))
                                        .await
                                        .unwrap();
                                    while let Ok(Some(message)) = datagrams.next_bincode().await {
                                        sender
                                            .send(MessageToSupervisor::FromFollower(
                                                introduction.name.clone(),
                                                message,
                                            ))
                                            .await
                                            .unwrap();
                                    }
                                }
                            }
                            Err(_e) => { /* connection failed */ }
                        }
                        Result::<(), anyhow::Error>::Ok(())
                    });
                }
                Result::<(), anyhow::Error>::Ok(())
            }
        });

        let state_updater = frontend_state_updater.clone();

        task::spawn({
            let sender = sender.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    let _ = sender.try_send(MessageToSupervisor::PokeServers);
                    interval.tick().await;
                }
            }
        });

        task::spawn(async move {
            let audio_output_stream_handle = {
                let (_audio_output_stream, audio_output_stream_handle) =
                    OutputStream::try_default().unwrap();
                std::mem::forget(_audio_output_stream);
                audio_output_stream_handle
            };
            let local_follower =
                SupervisedFollower::new(LocalFollower::new(audio_output_stream_handle));
            let mut this = Supervisor {
                start_time,
                total_inputs: 0,
                local_follower,
                remote_followers: HashMap::new(),
                active_follower_id: FollowerId::Local,
                servers: server_addresses
                    .iter()
                    .map(|&address| SupervisedServer {
                        address,
                        latest_run_id: None,
                        old_run_ids: vec![],
                        latest_received_sample_index: 0,
                        remote_time_estimator: RemoteTimeEstimator::default(),
                        signals: Default::default(),
                    })
                    .collect(),
                server_socket,
                frontend_state_updater,
                receiver,
                enabled: false,
                mouse_pressed: false,

                fft_planner: FftPlanner::new(),
                inputs_since_scroll_start: 0,
            };

            while let Some(message) = this.receiver.recv().await {
                this.handle_message(message);
            }
        });

        crate::webserver_glue::launch(
            state_updater,
            sender,
            PathBuf::from("web_frontend"),
            gui_port,
        )
        .await
    }
}
