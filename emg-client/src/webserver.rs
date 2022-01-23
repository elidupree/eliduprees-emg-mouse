use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageFromFrontend {
    SetEnabled(bool),
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FrontendState {
    pub enabled: bool,
    pub history: VecDeque<f64>,
}
