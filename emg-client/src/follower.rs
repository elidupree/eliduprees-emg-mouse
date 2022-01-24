use crate::utils::{load_sound, LoadedSound};
use enigo::{Enigo, MouseButton, MouseControllable};
use rodio::source::Buffered;
use rodio::{OutputStream, OutputStreamHandle};

pub enum MessageToFollower {
    Mousedown,
    MouseUp,
}

pub struct Follower {
    enigo: Enigo,
    _audio_output_stream: OutputStream,
    audio_output_stream_handle: OutputStreamHandle,
    click_sound: Buffered<LoadedSound>,
    unclick_sound: Buffered<LoadedSound>,
}

impl Follower {
    pub fn new() -> Follower {
        let (_audio_output_stream, audio_output_stream_handle) =
            OutputStream::try_default().unwrap();
        let enigo = Enigo::new();

        let click_sound = load_sound("../media/click.wav");
        let unclick_sound = load_sound("../media/unclick.wav");
        Follower {
            enigo,
            _audio_output_stream,
            audio_output_stream_handle,
            click_sound,
            unclick_sound,
        }
    }

    pub fn handle_message(&mut self, message: MessageToFollower) {
        match message {
            MessageToFollower::Mousedown => self.mousedown(),
            MessageToFollower::MouseUp => self.mouse_up(),
        }
    }

    pub fn mousedown(&mut self) {
        self.enigo.mouse_down(MouseButton::Left);
        self.audio_output_stream_handle
            .play_raw(self.click_sound.clone())
            .unwrap();
    }

    pub fn mouse_up(&mut self) {
        self.enigo.mouse_up(MouseButton::Left);
        self.audio_output_stream_handle
            .play_raw(self.unclick_sound.clone())
            .unwrap();
    }
}
