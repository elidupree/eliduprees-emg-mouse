use crate::remote_time_estimator::RemoteTimeEstimator;
use crate::utils::{load_sound, LoadedSound};
use enigo::{Enigo, MouseButton, MouseControllable};
use rodio::source::Buffered;
use rodio::{OutputStream, OutputStreamHandle};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, Write};
use std::net::TcpStream;
use std::ops::{Deref, DerefMut};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageToFollower {
    Mousedown,
    MouseUp,
    ScrollY(i32),
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageFromFollower {
    MouseMoved { time_since_start: Duration },
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FollowerIntroduction {
    pub name: String,
}

pub struct LocalFollower {
    enigo: Enigo,
    _audio_output_stream: OutputStream,
    audio_output_stream_handle: OutputStreamHandle,
    click_sound: Buffered<LoadedSound>,
    unclick_sound: Buffered<LoadedSound>,
    most_recent_mouse_location: (i32, i32),
}

pub struct RemoteFollower {
    stream: BufWriter<TcpStream>,
    remote_time_estimator: RemoteTimeEstimator,
}

pub trait Follower {
    fn handle_message(&mut self, message: MessageToFollower) {
        match message {
            MessageToFollower::Mousedown => self.mousedown(),
            MessageToFollower::MouseUp => self.mouse_up(),
            MessageToFollower::ScrollY(length) => self.scroll_y(length),
        }
    }

    fn mousedown(&mut self) {
        self.handle_message(MessageToFollower::Mousedown)
    }
    fn mouse_up(&mut self) {
        self.handle_message(MessageToFollower::MouseUp)
    }
    fn scroll_y(&mut self, length: i32) {
        self.handle_message(MessageToFollower::ScrollY(length))
    }
}

pub struct SupervisedFollower<F> {
    pub follower: F,
    pub most_recent_mouse_move: Instant,
}

pub enum SupervisedFollowerMut<'a> {
    Local(&'a mut SupervisedFollower<LocalFollower>),
    Remote(&'a mut SupervisedFollower<RemoteFollower>),
}

impl Follower for LocalFollower {
    fn mousedown(&mut self) {
        self.enigo.mouse_down(MouseButton::Left);
        self.audio_output_stream_handle
            .play_raw(self.click_sound.clone())
            .unwrap();
    }

    fn mouse_up(&mut self) {
        self.enigo.mouse_up(MouseButton::Left);
        self.audio_output_stream_handle
            .play_raw(self.unclick_sound.clone())
            .unwrap();
    }

    fn scroll_y(&mut self, length: i32) {
        self.enigo.mouse_scroll_y(length);
    }
}
impl Follower for RemoteFollower {
    fn handle_message(&mut self, message: MessageToFollower) {
        let _ = bincode::serialize_into(&mut self.stream, &message);
        let _ = self.stream.flush();
    }
}

impl LocalFollower {
    pub fn new() -> LocalFollower {
        let (_audio_output_stream, audio_output_stream_handle) =
            OutputStream::try_default().unwrap();
        let enigo = Enigo::new();

        let click_sound = load_sound("../media/click.wav");
        let unclick_sound = load_sound("../media/unclick.wav");
        LocalFollower {
            enigo,
            _audio_output_stream,
            audio_output_stream_handle,
            click_sound,
            unclick_sound,
            most_recent_mouse_location: (-1, -1),
        }
    }

    pub fn listen_to_remote(mut self, supervisor_address: &str, name: String) {
        let supervisor_stream = TcpStream::connect(supervisor_address).unwrap();
        let (sender_from_supervisor, receiver_from_supervisor) = mpsc::channel();
        std::thread::spawn({
            let mut supervisor_stream = BufReader::new(supervisor_stream.try_clone().unwrap());
            move || {
                while let Ok(message) =
                    bincode::deserialize_from::<_, MessageToFollower>(&mut supervisor_stream)
                {
                    sender_from_supervisor.send(message).unwrap();
                }
            }
        });
        let mut supervisor_stream = BufWriter::new(supervisor_stream);
        bincode::serialize_into(&mut supervisor_stream, &FollowerIntroduction { name }).unwrap();
        supervisor_stream.flush().unwrap();
        let start = Instant::now();
        loop {
            while let Ok(message) = receiver_from_supervisor.try_recv() {
                self.handle_message(message);
            }
            if let Some(_update) = self.update_most_recent_mouse_move() {
                let now = Instant::now();
                let time_since_start = now - start;
                bincode::serialize_into(
                    &mut supervisor_stream,
                    &MessageFromFollower::MouseMoved { time_since_start },
                )
                .unwrap();
                supervisor_stream.flush().unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn update_most_recent_mouse_move(&mut self) -> Option<Instant> {
        let new_location = self.enigo.mouse_location();
        let result = (new_location != self.most_recent_mouse_location).then(|| Instant::now());
        self.most_recent_mouse_location = new_location;
        result
    }
}

impl RemoteFollower {
    pub fn new(stream: TcpStream) -> RemoteFollower {
        RemoteFollower {
            stream: BufWriter::new(stream),
            remote_time_estimator: RemoteTimeEstimator::new(Duration::from_micros(500)),
        }
    }
}

impl<F: Follower> SupervisedFollower<F> {
    pub fn new(follower: F) -> Self {
        SupervisedFollower {
            follower,
            most_recent_mouse_move: Instant::now(),
        }
    }
    pub fn most_recent_mouse_move(&mut self) -> Instant {
        self.most_recent_mouse_move
    }
}

impl SupervisedFollower<LocalFollower> {
    pub fn update_most_recent_mouse_move(&mut self) {
        if let Some(update) = self.follower.update_most_recent_mouse_move() {
            self.most_recent_mouse_move = update;
        }
    }
}

impl SupervisedFollower<RemoteFollower> {
    pub fn observe_message(&mut self, remote_time_since_start: Duration, received_by: Instant) {
        self.follower
            .remote_time_estimator
            .observe(remote_time_since_start.as_secs_f64(), received_by);
    }
    pub fn remote_mouse_moved(&mut self, remote_time_since_start: Duration) {
        self.most_recent_mouse_move = self
            .follower
            .remote_time_estimator
            .estimate_local_time(remote_time_since_start.as_secs_f64());
    }
}

impl<'a> Deref for SupervisedFollowerMut<'a> {
    type Target = dyn Follower;

    fn deref(&self) -> &Self::Target {
        match self {
            SupervisedFollowerMut::Local(f) => &f.follower,
            SupervisedFollowerMut::Remote(f) => &f.follower,
        }
    }
}
impl<'a> DerefMut for SupervisedFollowerMut<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            SupervisedFollowerMut::Local(f) => &mut f.follower,
            SupervisedFollowerMut::Remote(f) => &mut f.follower,
        }
    }
}

impl<'a> SupervisedFollowerMut<'a> {
    pub fn most_recent_mouse_move(&mut self) -> Instant {
        match self {
            SupervisedFollowerMut::Local(f) => f.most_recent_mouse_move,
            SupervisedFollowerMut::Remote(f) => f.most_recent_mouse_move,
        }
    }
}
