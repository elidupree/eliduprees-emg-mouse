use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub enum MessageFromFrontend {}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct FrontendState {
    pub history: VecDeque<f64>,
}
