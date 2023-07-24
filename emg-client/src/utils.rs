use async_trait::async_trait;
use atomicbox::AtomicOptionBox;
use bytes::{BufMut, BytesMut};
use rodio::source::Buffered;
use rodio::{Decoder, Source};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::LazyLock;
use std::sync::{Arc, RwLock};
use tokio_stream::StreamExt;

static VARIABLES: LazyLock<RwLock<HashMap<String, f64>>> = LazyLock::new(|| {
    RwLock::new({
        [
            ("max_activity_contribution_per_frequency", 8.0),
            ("activity_threshold", 60.0),
            ("incremental_reduction_per_frame", 1.0 / 250.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    })
});

pub fn set_variable(key: &str, value: f64) {
    *VARIABLES.write().unwrap().get_mut(key).unwrap() = value;
}

pub fn get_variable(key: &str) -> f64 {
    VARIABLES.read().unwrap()[key]
}

pub fn get_variables() -> HashMap<String, f64> {
    VARIABLES.read().unwrap().clone()
}

pub struct LatestSender<T> {
    atom: Arc<AtomicOptionBox<T>>,
}

impl<T> LatestSender<T> {
    pub fn send(&self, value: T) {
        self.atom.swap(Some(Box::new(value)), Ordering::AcqRel);
    }
}

pub struct LatestReceiver<T> {
    atom: Arc<AtomicOptionBox<T>>,
    current: Option<T>,
}

impl<T> LatestReceiver<T> {
    pub fn current(&mut self) -> &mut Option<T> {
        // note: can be changed to Acquire if https://github.com/jorendorff/atomicbox/issues/9 is fixed
        if let Some(new) = self.atom.take(Ordering::AcqRel) {
            self.current = Some(*new);
        }
        &mut self.current
    }
}

pub fn latest_channel<T>() -> (LatestSender<T>, LatestReceiver<T>) {
    let atom = Arc::new(AtomicOptionBox::none());
    (
        LatestSender { atom: atom.clone() },
        LatestReceiver {
            atom,
            current: None,
        },
    )
}

pub fn load_sound(path: impl AsRef<Path>) -> Buffered<LoadedSound> {
    Decoder::new(BufReader::new(File::open(path).unwrap()))
        .unwrap()
        .convert_samples()
        .buffered()
}

pub type LoadedSound = impl Source<Item = f32>;

#[async_trait]
pub trait ConnectionExt {
    fn send_bincode_datagram<S: Serialize>(&self, message: &S) -> anyhow::Result<()>;
    async fn send_bincode_oneshot_stream<S: Serialize + Sync>(
        &self,
        message: &S,
    ) -> anyhow::Result<()>;
}

#[async_trait]
impl ConnectionExt for quinn::Connection {
    fn send_bincode_datagram<S: Serialize>(&self, message: &S) -> anyhow::Result<()> {
        let mut buf =
            BytesMut::with_capacity(bincode::serialized_size(&message)?.try_into()?).writer();
        bincode::serialize_into(&mut buf, &message)?;
        self.send_datagram(buf.into_inner().freeze())?;
        Ok(())
    }

    async fn send_bincode_oneshot_stream<S: Serialize + Sync>(
        &self,
        message: &S,
    ) -> anyhow::Result<()> {
        let buf = bincode::serialize(&message)?;
        let mut stream = self.open_uni().await?;
        stream.write(&buf).await?;
        Ok(())
    }
}

#[async_trait]
pub trait DatagramsExt {
    async fn next_bincode<T: DeserializeOwned>(&mut self) -> anyhow::Result<Option<T>>;
}

#[async_trait]
impl DatagramsExt for quinn::Datagrams {
    async fn next_bincode<T: DeserializeOwned>(&mut self) -> anyhow::Result<Option<T>> {
        match self.next().await {
            Some(buffer) => Ok(Some(bincode::deserialize(&buffer?)?)),
            None => Ok(None),
        }
    }
}

#[async_trait]
pub trait IncomingUniStreamsExt {
    async fn next_bincode_oneshot<T: DeserializeOwned>(&mut self) -> anyhow::Result<Option<T>>;
}

#[async_trait]
impl IncomingUniStreamsExt for quinn::IncomingUniStreams {
    async fn next_bincode_oneshot<T: DeserializeOwned>(&mut self) -> anyhow::Result<Option<T>> {
        match self.next().await {
            Some(stream) => Ok(Some(bincode::deserialize(
                &stream?.read_to_end(1_000_000).await?,
            )?)),
            None => Ok(None),
        }
    }
}
