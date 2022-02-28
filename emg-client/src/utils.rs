use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use rodio::source::Buffered;
use rodio::{Decoder, Source};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::convert::TryInto;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tokio_stream::StreamExt;

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
