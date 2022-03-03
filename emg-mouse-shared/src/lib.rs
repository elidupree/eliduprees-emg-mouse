use serde::{Deserialize, Serialize};
use std::time::Duration;

pub type ServerRunId = u64;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub struct ReportFromServer<SamplesArray> {
    pub server_run_id: ServerRunId,
    pub latest_sample_index: u64,
    pub samples: SamplesArray,
}
pub type OwnedSamplesArray = Vec<Samples>;
pub type BorrowedSamplesArray<'a> = &'a [Samples];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub struct MessageToServer {
    pub server_run_id: ServerRunId,
    pub latest_received_sample_index: u64,
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub struct Samples {
    pub time_since_start: Duration,
    pub inputs: [u16; 4],
}
