#![feature(array_zip)]

use crate::face_position_model::FacePositionModel;
use nalgebra::Vector2;
use std::sync::Arc;

mod face_position_model;
mod utils;

fn main() {
    let mut current_model: Option<FacePositionModel> = None;
    let lines = std::io::stdin().lines();
    for line in lines {
        let line = line.unwrap();
        let camera_landmarks: Arc<[Vector2<f64>]> = serde_json::from_str(&line).unwrap();
        if let Some(current_model) = current_model.as_mut() {
            *current_model = current_model.conformed_to(camera_landmarks);
        } else {
            current_model = Some(FacePositionModel::default_from_camera(&camera_landmarks));
        }
    }
}
