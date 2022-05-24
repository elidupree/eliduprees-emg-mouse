use crate::bluetooth::messages_from_server;
use crate::bluetooth::ReportFromServer;
use crate::follower::{
    FollowerIntroduction, LocalFollower, MessageFromFollower, RemoteFollower, SupervisedFollower,
    SupervisedFollowerMut,
};
use crate::remote_time_estimator::RemoteTimeEstimator;
use crate::signal::Signal;
use crate::utils::{DatagramsExt, IncomingUniStreamsExt};
use crate::webserver::{FrontendState, MessageFromFrontend};
use actix::{Actor, Context, Handler, Message};
use anyhow::{bail, Context as _};
use crossbeam::atomic::AtomicCell;
use log::info;
use rodio::OutputStream;
//use rustfft::FftPlanner;
use statrs::statistics::Statistics;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

pub struct SupervisedServer {
    server_run_id: u64,
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

    frontend_state_updater: Arc<AtomicCell<Option<FrontendState>>>,

    enabled: bool,
    mouse_pressed: bool,
    inputs_since_scroll_start: usize,
    //fft_planner: FftPlanner<f64>,
}

impl Actor for Supervisor {
    type Context = Context<Self>;
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct MessageFromServer {
    server_index: usize,
    local_time_received: Instant,
    report: ReportFromServer,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct ServerReconnected {
    server_index: usize,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct NewFollower {
    name: String,
    follower: SupervisedFollower<RemoteFollower>,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct MessageFromIdentifiedFollower {
    name: String,
    message: MessageFromFollower,
}

impl SupervisedServer {
    fn reconnected(&mut self) {
        self.signals = Default::default();
        self.remote_time_estimator = RemoteTimeEstimator::default();
    }
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
            histories: self
                .servers
                .iter()
                .flat_map(|server| {
                    server.signals.iter().map(|s| {
                        s.history
                            .iter()
                            .filter(|f| f.time >= latest_time - 0.8)
                            .cloned()
                            .collect()
                    })
                })
                .collect(),
            frequencies_histories: self
                .servers
                .iter()
                .flat_map(|server| server.signals.iter().map(|s| s.frequencies_history.clone()))
                .collect(),
        }));
    }
}

impl Handler<NewFollower> for Supervisor {
    type Result = ();

    fn handle(&mut self, message: NewFollower, _context: &mut Self::Context) -> Self::Result {
        self.remote_followers.insert(message.name, message.follower);
    }
}

impl Handler<MessageFromFrontend> for Supervisor {
    type Result = ();

    fn handle(
        &mut self,
        message: MessageFromFrontend,
        _context: &mut Self::Context,
    ) -> Self::Result {
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
}

impl Handler<MessageFromIdentifiedFollower> for Supervisor {
    type Result = ();

    fn handle(
        &mut self,
        message: MessageFromIdentifiedFollower,
        _context: &mut Self::Context,
    ) -> Self::Result {
        let MessageFromIdentifiedFollower { name, message } = message;
        match message {
            MessageFromFollower::MouseMoved { time_since_start } => {
                let now = Instant::now();
                let follower = self.remote_followers.get_mut(&name).unwrap();
                follower.observe_message(time_since_start, now);
                follower.remote_mouse_moved(time_since_start);
            }
        }
    }
}

impl Handler<ServerReconnected> for Supervisor {
    type Result = ();

    fn handle(&mut self, message: ServerReconnected, _context: &mut Self::Context) -> Self::Result {
        let ServerReconnected { server_index } = message;
        self.servers[server_index].reconnected();
    }
}
impl Handler<MessageFromServer> for Supervisor {
    type Result = ();

    fn handle(&mut self, message: MessageFromServer, _context: &mut Self::Context) -> Self::Result {
        let MessageFromServer {
            server_index,
            local_time_received,
            report,
        } = message;
        if self.servers[server_index].server_run_id != report.server_run_id {
            self.servers[server_index].server_run_id = report.server_run_id;
            self.servers[server_index].reconnected();
        }
        self.servers[server_index].remote_time_estimator.observe(
            (report.first_sample_index + report.samples.len() as u64 - 1) as f64,
            local_time_received,
        );
        self.local_follower.update_most_recent_mouse_move();
        self.update_active_follower();

        for (sample_index_within_report, inputs) in report.samples.iter().enumerate() {
            let _average = inputs.iter().map(|&i| i as f64).mean();

            let mouse_active_before = self.servers[server_index].signals[2].is_active();
            for (signal, &input) in self.servers[server_index].signals.iter_mut().zip(inputs) {
                signal.receive_raw(
                    input as f64, /*- average*/
                    (report.first_sample_index + sample_index_within_report as u64) as f64 / 1000.0,
                    //&mut self.fft_planner,
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

            self.update_frontend();
            self.total_inputs += 1;
            // println!(
            //     "{}: {:.2}, {}",
            //     self.total_inputs,
            //     self.total_inputs as f64 / report.time_since_start.as_secs_f64(),
            //     report.time_since_start.as_micros(),
            // );
        }
    }
}

impl Supervisor {
    pub async fn run(
        SupervisorOptions {
            server_address,
            gui_port,
            follower_port,
        }: SupervisorOptions,
    ) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let frontend_state_updater = Arc::new(AtomicCell::new(None));

        let server_addresses = [server_address.parse::<SocketAddr>().unwrap()];

        let audio_output_stream_handle = {
            let (_audio_output_stream, audio_output_stream_handle) =
                OutputStream::try_default().unwrap();
            std::mem::forget(_audio_output_stream);
            audio_output_stream_handle
        };
        let local_follower =
            SupervisedFollower::new(LocalFollower::new(audio_output_stream_handle));
        let supervisor = Supervisor {
            start_time,
            total_inputs: 0,
            local_follower,
            remote_followers: HashMap::new(),
            active_follower_id: FollowerId::Local,
            servers: server_addresses
                .iter()
                .map(|&_address| SupervisedServer {
                    server_run_id: 0,
                    remote_time_estimator: RemoteTimeEstimator::default(),
                    signals: Default::default(),
                })
                .collect(),
            frontend_state_updater: frontend_state_updater.clone(),
            enabled: false,
            mouse_pressed: false,

            //fft_planner: FftPlanner::new(),
            inputs_since_scroll_start: 0,
        }
        .start();

        task::spawn({
            let supervisor = supervisor.clone();
            async move {
                let mut stream = messages_from_server();
                while let Some(report) = stream.next().await {
                    let local_time_received = Instant::now();
                    supervisor.do_send(MessageFromServer {
                        server_index: 0,
                        local_time_received,
                        report,
                    })
                }
            }
        });

        // for (server_index, server_address) in server_addresses.iter().cloned().enumerate() {
        //     let supervisor = supervisor.clone();
        //     task::spawn(async move {
        //         let report_size =
        //             bincode::serialized_size(&ReportFromServer::default()).unwrap() as usize;
        //         let mut buffer = vec![0u8; report_size];
        //         loop {
        //             let mut server_stream =
        //                 match timeout(Duration::from_secs(2), TcpStream::connect(server_address))
        //                     .await
        //                 {
        //                     Ok(Ok(server_stream)) => server_stream,
        //                     Ok(Err(e)) => {
        //                         eprintln!("Server TcpStream connection error: {}", e);
        //                         continue;
        //                     }
        //                     Err(_) => {
        //                         eprintln!("Server TcpStream connection timed out");
        //                         continue;
        //                     }
        //                 };
        //             supervisor.do_send(ServerReconnected { server_index });
        //             loop {
        //                 match timeout(
        //                     Duration::from_secs(2),
        //                     server_stream.read_exact(&mut buffer),
        //                 )
        //                 .await
        //                 {
        //                     Ok(Ok(s)) => {
        //                         let local_time_received = Instant::now();
        //                         assert_eq!(s, report_size);
        //                         match bincode::deserialize(&buffer) {
        //                             Ok(report) => supervisor.do_send(MessageFromServer {
        //                                 server_index,
        //                                 local_time_received,
        //                                 report,
        //                             }),
        //                             Err(e) => {
        //                                 eprintln!("Bincode error reading from server: {}", e);
        //                             }
        //                         }
        //                     }
        //                     Ok(Err(e)) => {
        //                         eprintln!("IO error reading from server: {}", e);
        //                         break;
        //                     }
        //                     Err(_) => {
        //                         eprintln!("Timed out reading from server");
        //                         break;
        //                     }
        //                 }
        //             }
        //         }
        //     });
        // }

        task::spawn({
            let supervisor = supervisor.clone();
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
                    let supervisor = supervisor.clone();
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
                                    supervisor.do_send(NewFollower {
                                        name: introduction.name.clone(),
                                        follower: SupervisedFollower::new(RemoteFollower::new(
                                            connection,
                                        )),
                                    });
                                    while let Ok(Some(message)) = datagrams.next_bincode().await {
                                        let _ =
                                            supervisor.try_send(MessageFromIdentifiedFollower {
                                                name: introduction.name.clone(),
                                                message,
                                            });
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

        crate::webserver_glue::launch(
            frontend_state_updater,
            supervisor,
            PathBuf::from("web_frontend"),
            gui_port,
        )
        .await
    }
}
