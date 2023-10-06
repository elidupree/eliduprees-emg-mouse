#[cfg(feature = "bluetooth")]
use crate::bluetooth::{messages_from_server, ReportFromServer};
use crate::follower::{
    FollowerIntroduction, LocalFollower, MessageFromFollower, RemoteFollower, SupervisedFollower,
    SupervisedFollowerMut,
};
use crate::remote_time_estimator::RemoteTimeEstimator;
#[cfg(not(feature = "bluetooth"))]
use crate::serial_port_communication::{messages_from_server, ReportFromServer};
use crate::signal::Signal;
use crate::utils::{DatagramsExt, IncomingUniStreamsExt};
use crate::webserver::{MessageFromFrontend, MessageToFrontend};
use actix::{Actor, Addr, Context, Handler, Message};
use anyhow::{bail, Context as _};
use log::info;
use rodio::OutputStream;
//use rustfft::FftPlanner;
use crate::webserver_glue::FrontendSession;
use async_bincode::{AsyncBincodeReader, AsyncBincodeWriter};
use itertools::multizip;
use statrs::statistics::Statistics;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
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

    frontend_session: Option<Addr<FrontendSession>>,

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
pub struct NewFrontendSession {
    pub session: Addr<FrontendSession>,
}

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct MessageFromIdentifiedFollower {
    name: String,
    message: MessageFromFollower,
}

trait NotifyOptionExt {
    fn notify(&mut self, message: MessageToFrontend);
}

impl NotifyOptionExt for Option<Addr<FrontendSession>> {
    fn notify(&mut self, message: MessageToFrontend) {
        if let Some(session) = self {
            session.do_send(message);
        }
    }
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
    // fn update_frontend(&mut self) {
    //     let start_time = self.start_time;
    //     let latest_time = self.servers[0].signals[0]
    //         .history
    //         .back()
    //         .map_or(0.0, |f| f.time);
    //     self.frontend_session.store(Some(FrontendState {
    //         enabled: self.enabled,
    //         followers: std::iter::once((
    //             "Local".to_string(),
    //             (self.local_follower.most_recent_mouse_move() - start_time).as_secs_f64(),
    //         ))
    //         .chain(self.remote_followers.iter_mut().map(|(n, f)| {
    //             (
    //                 n.clone(),
    //                 (f.most_recent_mouse_move() - start_time).as_secs_f64(),
    //             )
    //         }))
    //         .collect(),
    //     }));
    // }
}

impl Handler<NewFollower> for Supervisor {
    type Result = ();

    fn handle(&mut self, message: NewFollower, _context: &mut Self::Context) -> Self::Result {
        self.remote_followers.insert(message.name, message.follower);
    }
}

impl Handler<NewFrontendSession> for Supervisor {
    type Result = ();

    fn handle(
        &mut self,
        message: NewFrontendSession,
        _context: &mut Self::Context,
    ) -> Self::Result {
        message.session.do_send(MessageToFrontend::Initialize {
            enabled: self.enabled,
            variables: crate::utils::get_variables(),
        });
        self.frontend_session = Some(message.session);
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
            }
            MessageFromFrontend::SetVariable(key, value) => crate::utils::set_variable(&key, value),
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
                self.frontend_session
                    .notify(MessageToFrontend::UpdateFollower {
                        name: name.clone(),
                        latest_move_time: (follower.most_recent_mouse_move() - self.start_time)
                            .as_secs_f64(),
                    });
                //dbg!(&follower.follower);
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
        self.frontend_session
            .notify(MessageToFrontend::UpdateFollower {
                name: "Local".to_string(),
                latest_move_time: (self.local_follower.most_recent_mouse_move() - self.start_time)
                    .as_secs_f64(),
            });

        self.update_active_follower();

        let mut new_history_frames = [const { Vec::new() }; 4];
        let mut new_frequencies_frames = [const { Vec::new() }; 4];

        for (sample_index_within_report, inputs) in report.samples.iter().enumerate() {
            let _average = inputs.iter().map(|&i| i as f64).mean();

            let mouse_active_before = self.servers[server_index].signals[2].is_active();
            for (signal, &input, new_history_frames, new_frequencies_frames) in multizip((
                &mut self.servers[server_index].signals,
                inputs,
                &mut new_history_frames,
                &mut new_frequencies_frames,
            )) {
                signal.receive_raw(
                    input as f64, /*- average*/
                    (report.first_sample_index + sample_index_within_report as u64) as f64 / 1020.0,
                    //&mut self.fft_planner,
                    |f| new_history_frames.push(f),
                    |f| new_frequencies_frames.push(f),
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
                    let denom = 300 * s;
                    (inputs * s + inputs * inputs + denom - 1) / denom
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
            // println!(
            //     "{}: {:.2}, {}",
            //     self.total_inputs,
            //     self.total_inputs as f64 / report.time_since_start.as_secs_f64(),
            //     report.time_since_start.as_micros(),
            // );
        }
        if !new_history_frames[0].is_empty() {
            self.frontend_session
                .notify(MessageToFrontend::NewHistoryFrames {
                    server_index,
                    frames: new_history_frames,
                });
        }
        if !new_frequencies_frames[0].is_empty() {
            self.frontend_session
                .notify(MessageToFrontend::NewFrequenciesFrames {
                    server_index,
                    frames: new_frequencies_frames,
                });
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
            frontend_session: None,
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

                // let (_endpoint, mut incoming) =
                //     quinn::Endpoint::server(server_config, ([0, 0, 0, 0], follower_port).into())?;
                let listener = TcpListener::bind(("0.0.0.0", follower_port)).await.unwrap();

                // while let Some(connection) = incoming.next().await {
                while let Ok((stream, _addr)) = listener.accept().await {
                    dbg!();
                    let supervisor = supervisor.clone();
                    task::spawn(async move {
                        // match connection.await {
                        //     Ok(quinn::NewConnection {
                        //         connection,
                        //         mut uni_streams,
                        //         mut datagrams,
                        //         ..
                        //     }) => {
                        //         if let Ok(Some(introduction)) = uni_streams
                        //             .next_bincode_oneshot::<FollowerIntroduction>()
                        //             .await
                        //         {
                        //             supervisor.do_send(NewFollower {
                        //                 name: introduction.name.clone(),
                        //                 follower: SupervisedFollower::new(RemoteFollower::new(
                        //                     connection,
                        //                 )),
                        //             });
                        //             while let Ok(Some(message)) = datagrams.next_bincode().await {
                        //                 let _ =
                        //                     supervisor.try_send(MessageFromIdentifiedFollower {
                        //                         name: introduction.name.clone(),
                        //                         message,
                        //                     });
                        //             }
                        //         }
                        //     }
                        //     Err(_e) => { /* connection failed */ }
                        // }

                        let (mut read_half, write_half) = stream.into_split();
                        let size = read_half.read_u32().await?;
                        let mut introduction_buf = vec![0; size as usize];
                        read_half.read_exact(&mut introduction_buf).await?;
                        let write_stream = AsyncBincodeWriter::from(write_half).for_async();
                        let mut read_stream: AsyncBincodeReader<_, MessageFromFollower> =
                            AsyncBincodeReader::from(read_half);
                        if let Ok(introduction) =
                            bincode::deserialize::<FollowerIntroduction>(&introduction_buf)
                        {
                            supervisor.do_send(NewFollower {
                                name: introduction.name.clone(),
                                follower: SupervisedFollower::new(RemoteFollower::new(
                                    write_stream,
                                )),
                            });
                            while let Some(Ok(message)) = read_stream.next().await {
                                let _ = supervisor.try_send(MessageFromIdentifiedFollower {
                                    name: introduction.name.clone(),
                                    message,
                                });
                            }
                        }

                        Result::<(), anyhow::Error>::Ok(())
                    });
                }
                Result::<(), anyhow::Error>::Ok(())
            }
        });

        crate::webserver_glue::launch(supervisor, PathBuf::from("web_frontend"), gui_port).await
    }
}
