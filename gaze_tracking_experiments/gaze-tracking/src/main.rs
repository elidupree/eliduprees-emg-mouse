#![feature(array_zip)]

use crate::face_position_model::FacePositionModel;
use kiss3d::window::Window;
use nalgebra::Vector2;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

mod face_position_model;
mod utils;

fn main() {
    let mut child = Command::new("python")
        .args(["../main.py"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let child_output = BufReader::new(child.stdout.take().unwrap());
    let mut current_model: Option<FacePositionModel> = None;

    let mut window = Window::new("EliDupree's EMG Mouse Gaze Tracker Viz");
    for line in child_output.lines() {
        let line = line.unwrap();
        let camera_landmarks: Arc<[Vector2<f64>]> = serde_json::from_str(&line).unwrap();
        if let Some(current_model) = current_model.as_mut() {
            *current_model = current_model.conformed_to(camera_landmarks);
            current_model.draw(&mut window);
        } else {
            current_model = Some(FacePositionModel::default_from_camera(&camera_landmarks));
        }
        window.render();
    }
}
