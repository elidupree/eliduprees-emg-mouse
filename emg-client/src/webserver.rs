use actix::Message;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug, Message)]
#[rtype(result = "()")]
pub enum MessageFromFrontend {
    SetEnabled(bool),
    SetVariable(String, f64),
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug, Message)]
#[rtype(result = "()")]
pub enum MessageToFrontend {
    Initialize {
        enabled: bool,
        variables: HashMap<String, f64>,
    },
    NewHistoryFrames {
        server_index: usize,
        frames: [Vec<HistoryFrame>; 4],
    },
    NewFrequenciesFrames {
        server_index: usize,
        frames: [Vec<FrequenciesFrame>; 4],
    },
    UpdateFollower {
        name: String,
        latest_move_time: f64,
    },
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct HistoryFrame {
    pub time: f64,
    pub value: f64,
    pub activity_threshold: f64,
    pub too_much_threshold: f64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FrequenciesFrame {
    pub time: f64,
    pub values: Vec<Vec<f64>>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FrontendState {
    pub enabled: bool,
    pub followers: Vec<(String, f64)>,
    pub histories: Vec<VecDeque<HistoryFrame>>,
    pub frequencies_histories: Vec<VecDeque<Vec<f64>>>,
}
