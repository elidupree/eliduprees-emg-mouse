use crate::utils::{load_sound, LoadedSound};
use crossbeam::atomic::AtomicCell;
use enigo::{Enigo, MouseButton, MouseControllable};
use rodio::source::Buffered;
use rodio::{OutputStream, OutputStreamHandle};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageToFollower {
    Mousedown,
    MouseUp,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageFromFollower {
    MouseMoved,
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
    stream: TcpStream,
    most_recent_mouse_move_updater: Arc<AtomicCell<Option<Instant>>>,
}

pub trait Follower {
    fn handle_message(&mut self, message: MessageToFollower) {
        match message {
            MessageToFollower::Mousedown => self.mousedown(),
            MessageToFollower::MouseUp => self.mouse_up(),
        }
    }

    fn mousedown(&mut self) {
        self.handle_message(MessageToFollower::Mousedown)
    }

    fn mouse_up(&mut self) {
        self.handle_message(MessageToFollower::MouseUp)
    }

    fn update_most_recent_mouse_move(&mut self) -> Option<Instant>;
}

pub struct SupervisedFollower<F> {
    follower: F,
    most_recent_mouse_move: Instant,
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

    fn update_most_recent_mouse_move(&mut self) -> Option<Instant> {
        let new_location = self.enigo.mouse_location();
        let result = (new_location != self.most_recent_mouse_location).then(|| Instant::now());
        self.most_recent_mouse_location = new_location;
        result
    }
}
impl Follower for RemoteFollower {
    fn handle_message(&mut self, message: MessageToFollower) {
        let _ = bincode::serialize_into(&self.stream, &message);
    }

    fn update_most_recent_mouse_move(&mut self) -> Option<Instant> {
        self.most_recent_mouse_move_updater.take()
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

    pub fn handle_message(&mut self, message: MessageToFollower) {
        match message {
            MessageToFollower::Mousedown => self.mousedown(),
            MessageToFollower::MouseUp => self.mouse_up(),
        }
    }

    pub fn listen_to_remote(mut self, supervisor_address: &str) {
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
        loop {
            while let Ok(message) = receiver_from_supervisor.try_recv() {
                self.handle_message(message);
            }
            if let Some(_update) = self.update_most_recent_mouse_move() {
                bincode::serialize_into(&mut supervisor_stream, &MessageFromFollower::MouseMoved)
                    .unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl<F: Follower> SupervisedFollower<F> {
    pub fn most_recent_mouse_move(&mut self) -> Instant {
        if let Some(new) = self.follower.update_most_recent_mouse_move() {
            self.most_recent_mouse_move = new;
        }
        self.most_recent_mouse_move.clone()
    }

    pub fn mousedown(&mut self) {
        self.follower.mousedown()
    }

    pub fn mouse_up(&mut self) {
        self.follower.mouse_up()
    }
}

impl<'a> SupervisedFollowerMut<'a> {
    pub fn most_recent_mouse_move(&mut self) -> Instant {
        match self {
            SupervisedFollowerMut::Local(f) => f.most_recent_mouse_move(),
            SupervisedFollowerMut::Remote(f) => f.most_recent_mouse_move(),
        }
    }

    pub fn mousedown(&mut self) {
        match self {
            SupervisedFollowerMut::Local(f) => f.mousedown(),
            SupervisedFollowerMut::Remote(f) => f.mousedown(),
        }
    }

    pub fn mouse_up(&mut self) {
        match self {
            SupervisedFollowerMut::Local(f) => f.mouse_up(),
            SupervisedFollowerMut::Remote(f) => f.mouse_up(),
        }
    }
}
