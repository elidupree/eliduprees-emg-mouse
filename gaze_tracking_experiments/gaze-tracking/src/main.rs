#![feature(array_zip)]
#![feature(default_free_fn)]

use crate::face_position_model::FacePositionModel;
use kiss3d::window::Window;
use nalgebra::{Matrix2xX, Vector2};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

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
        // let camera_landmarks: Matrix2xX<f64> = serde_json::from_str(&line).unwrap();
        let camera_landmarks: Vec<Vector2<f64>> = serde_json::from_str(&line).unwrap();
        let camera_landmarks = Matrix2xX::from_columns(&camera_landmarks);
        if let Some(current_model) = current_model.as_mut() {
            current_model.add_frame(camera_landmarks);
            current_model.draw(&mut window);
        } else {
            current_model = Some(FacePositionModel::default_from_camera(camera_landmarks));
        }
        window.render();
    }
}
