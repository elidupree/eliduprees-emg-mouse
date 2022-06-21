#![feature(array_zip)]
#![feature(default_free_fn)]

use crate::face_position_model::{AddFrameResults, FacePositionModel};
use kiss3d::window::Window;
use nalgebra::{Matrix2xX, Vector2};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

mod face_position_model;
mod utils;

fn main() {
    // let mut child = Command::new("python")
    //     .args(["../main.py"])
    //     .stdout(Stdio::piped())
    //     .spawn()
    //     .unwrap();
    // let child_output = BufReader::new(child.stdout.take().unwrap());
    // let lines = child_output.lines();

    let lines = BufReader::new(std::fs::File::open("../test_landmarks").unwrap()).lines();

    let mut current_model: Option<FacePositionModel> = None;
    let mut total_iterations = 0;
    let mut total_loss = 0.0;

    let mut window = Window::new("EliDupree's EMG Mouse Gaze Tracker Viz");
    for line in lines {
        let line = line.unwrap();
        // let camera_landmarks: Matrix2xX<f64> = serde_json::from_str(&line).unwrap();
        let camera_landmarks: Vec<Vector2<f64>> = serde_json::from_str(&line).unwrap();
        let camera_landmarks = Matrix2xX::from_columns(&camera_landmarks);
        if let Some(current_model) = current_model.as_mut() {
            let AddFrameResults {
                final_loss,
                iterations,
            } = current_model.add_frame(camera_landmarks);
            total_loss += final_loss;
            total_iterations += iterations;
            println!("Totals: {total_loss}, {total_iterations}");
            current_model.draw(&mut window);
        } else {
            current_model = Some(FacePositionModel::default_from_camera(camera_landmarks));
        }
        window.render();
    }
}
