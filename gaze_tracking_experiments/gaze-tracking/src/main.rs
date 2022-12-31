#![feature(array_zip, default_free_fn)]

use crate::face_position_model::{
    AddFrameResults, CameraLandmarks, FacePositionModel, MetaParameters,
};
use kiss3d::window::Window;
use nalgebra::Vector2;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Instant;
// use std::process::{Command, Stdio};

mod face_position_model;
mod utils;

fn run(
    window: &mut Window,
    all_camera_landmarks: &[Arc<CameraLandmarks>],
    parameters: &MetaParameters,
) -> (f64, usize) {
    let mut current_model: Option<FacePositionModel> = None;
    let mut total_iterations = 0;
    let mut total_loss = 0.0;
    for camera_landmarks in all_camera_landmarks {
        if let Some(current_model) = current_model.as_mut() {
            let AddFrameResults {
                final_loss,
                iterations,
            } = current_model.add_frame(parameters, camera_landmarks.clone());
            total_loss += final_loss;
            total_iterations += iterations;
            current_model.draw(window);
        } else {
            current_model = Some(FacePositionModel::default_from_camera(
                camera_landmarks.clone(),
                &[Vector2::new(3840.0, 2160.0), Vector2::new(3840.0, 2160.0)],
            ));
        }
        window.render();
        //std::thread::sleep(std::time::Duration::from_millis(500));
    }
    println!("Totals: {total_loss}, {total_iterations}");
    (total_loss, total_iterations)
}

fn main() {
    // let mut child = Command::new("python")
    //     .args(["../main.py"])
    //     .stdout(Stdio::piped())
    //     .spawn()
    //     .unwrap();
    // let child_output = BufReader::new(child.stdout.take().unwrap());
    // let lines = child_output.lines();

    let lines =
        BufReader::new(std::fs::File::open("../test_landmarks_with_eye_movement").unwrap()).lines();
    let all_camera_landmarks: Vec<_> = lines
        .map(|line| {
            let line = line.unwrap();
            let camera_landmarks: Vec<Vector2<f64>> = serde_json::from_str(&line).unwrap();
            Arc::new(CameraLandmarks::from_mediapipe_facemesh(camera_landmarks))
        })
        .collect();
    let mut window = Window::new("EliDupree's EMG Mouse Gaze Tracker Viz");

    if true {
        utils::start_recording_reports();
        run(&mut window, &all_camera_landmarks, &MetaParameters::new());
        utils::save_reports();
    } else {
        let start = Instant::now();
        let mut parameters = MetaParameters::new();
        let mut best_score = 999999999999999999.0;
        let num_runs = 100;
        for run_index in 0..num_runs {
            let new_parameters = if run_index == 0 {
                parameters.clone()
            } else {
                parameters.mutate(1.0 - (run_index as f64 / num_runs as f64))
            };
            let elapsed = start.elapsed();
            println!("Starting run {run_index} at {elapsed:?}, trying {new_parameters:?}");
            let (total_loss, total_iterations) =
                run(&mut window, &all_camera_landmarks, &new_parameters);
            let score = total_loss + total_iterations as f64 / 1_000.0;
            if score < best_score {
                best_score = score;
                parameters = new_parameters;
                println!("Improved!");
            }
        }
    }
}
