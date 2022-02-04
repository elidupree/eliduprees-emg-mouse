use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageFromFrontend {
    SetEnabled(bool),
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct HistoryFrame {
    pub time: f64,
    pub value: f64,
    pub click_threshold: f64,
    pub too_much_threshold: f64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FrontendState {
    pub enabled: bool,
    pub followers: Vec<(String, f64)>,
    pub histories: Vec<VecDeque<HistoryFrame>>,
    pub frequencies_history: VecDeque<Vec<f64>>,
}
