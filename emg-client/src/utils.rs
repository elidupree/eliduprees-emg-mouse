use rodio::source::Buffered;
use rodio::{Decoder, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub fn load_sound(path: impl AsRef<Path>) -> Buffered<LoadedSound> {
    Decoder::new(BufReader::new(File::open(path).unwrap()))
        .unwrap()
        .convert_samples()
        .buffered()
}
pub type LoadedSound = impl Source<Item = f32>;
