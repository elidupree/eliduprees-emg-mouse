use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const HEARTBEAT_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub struct ReportFromServer {
    pub time_since_start: Duration,
    pub inputs: [u16; 4],
}
