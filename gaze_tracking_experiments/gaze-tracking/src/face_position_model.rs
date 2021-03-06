use crate::utils;
use crate::utils::{matrix2_from_column_iter, matrix_from_column_iter, Vector3Ext};
use itertools::multizip;
use kiss3d::window::Window;
use nalgebra::{
    DVector, Matrix2, Matrix2xX, Matrix3x2, Matrix3xX, Unit, UnitQuaternion, UnitVector3, Vector2,
    Vector3, VectorSlice3,
};
use rand::prelude::*;
use std::iter::zip;
use std::sync::Arc;

pub struct CameraLandmarks {
    pub regular_landmarks: Matrix2xX<f64>,
    pub pupils: Matrix2<f64>,
}

const MEDIAPIPE_FACEMESH_LEFT_PUPIL_INDEX: usize = 468;
const MEDIAPIPE_FACEMESH_RIGHT_PUPIL_INDEX: usize = 473;

impl CameraLandmarks {
    pub fn from_mediapipe_facemesh(landmarks: Vec<Vector2<f64>>) -> Self {
        CameraLandmarks {
            regular_landmarks: matrix2_from_column_iter(
                landmarks
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| {
                        i != MEDIAPIPE_FACEMESH_LEFT_PUPIL_INDEX
                            && i != MEDIAPIPE_FACEMESH_RIGHT_PUPIL_INDEX
                    })
                    .map(|(_i, &l)| l),
            ),
            pupils: Matrix2::from_columns(&[
                landmarks[MEDIAPIPE_FACEMESH_LEFT_PUPIL_INDEX],
                landmarks[MEDIAPIPE_FACEMESH_RIGHT_PUPIL_INDEX],
            ]),
        }
    }
}

#[derive(Clone)]
struct Frame {
    time_index: usize,
    camera_landmarks: Arc<CameraLandmarks>,
    center_of_mass: Vector3<f64>,
    orientation: UnitQuaternion<f64>,
    eye_directions: [UnitVector3<f64>; 2],
}

#[derive(Clone)]
pub struct FacePositionModel {
    frames: Vec<Frame>,
    landmark_offsets: Matrix3xX<f64>,

    /// the "spatial depth units per spatial horizontal unit" at 1.0 planar units away from camera center
    /// units are "spatial depth units" * "planar units" / "spatial horizontal units"
    camera_fov_slope: Vector2<f64>,

    eye_center_offsets: Matrix3x2<f64>,
    eye_radii: Vector2<f64>,
}

#[derive(Clone)]
struct FrameAnalysis {
    loss: f64,
    d_loss_d_translation: Vector3<f64>,
    d_loss_d_rotation: Vector3<f64>,
    d_loss_d_eye_rotations: Matrix3x2<f64>,
    proposed_translation: Vector3<f64>,
    proposed_rotation: Vector3<f64>,
    proposed_eye_rotations: Matrix3x2<f64>,
}

#[derive(Clone)]
struct FacePositionModelAnalysis {
    frames: Vec<FrameAnalysis>,
    landmark_loss: DVector<f64>,
    loss: f64,
    d_loss_d_fov_slope: Vector2<f64>,
    d_loss_d_landmark_offsets: Matrix3xX<f64>,
    d_loss_d_eye_center_offsets: Matrix3x2<f64>,
    d_loss_d_eye_radius_changes: Vector2<f64>,
    proposed_fov_slope_change: Vector2<f64>,
    proposed_landmark_offsets_change: Matrix3xX<f64>,
    proposed_eye_center_offsets_change: Matrix3x2<f64>,
    proposed_eye_radius_changes: Vector2<f64>,
    d_loss_d_learning: f64,
}

#[derive(Clone, Debug)]
pub struct MetaParameters {
    translation_rate: f64,
    rotation_rate: f64,
    eye_rotation_rate: f64,
    reshaping_rate: f64,
    fov_tweak_rate: f64,
    eye_center_offsets_change_rate: f64,
    eye_radius_change_rate: f64,
    learning_smoothness_threshold_logistic_input: f64,
    learning_smoothness_threshold: f64,
    learning_increase_factor: f64,
    learning_decrease_factor_logistic_input: f64,
    learning_decrease_factor: f64,
}

impl MetaParameters {
    pub fn new() -> Self {
        // MetaParameters {
        //     translation_rate: 0.307,
        //     rotation_rate: 2.018,
        //     reshaping_rate: 1.542,
        //     fov_tweak_rate: 0.947,
        //     learning_smoothness_threshold_logistic_input: 0.0,
        //     learning_smoothness_threshold: 0.5,
        //     learning_increase_factor: 2.0,
        //     learning_decrease_factor_logistic_input: 0.0,
        //     learning_decrease_factor: 0.5,
        // };
        // MetaParameters {
        //     translation_rate: 0.2779278249261501,
        //     rotation_rate: 10.370492557651998,
        //     reshaping_rate: 1.3090546815318604,
        //     fov_tweak_rate: 1.2058344227743527,
        //     learning_smoothness_threshold_logistic_input: 0.8224878187339855,
        //     learning_smoothness_threshold: 0.8382108367011485,
        //     learning_increase_factor: 1.2646115090985655,
        //     learning_decrease_factor_logistic_input: -1.3916990906260678,
        //     learning_decrease_factor: 0.05822792800439669,
        // };
        MetaParameters {
            translation_rate: 0.2699628982930168,
            rotation_rate: 6.783023480987058,
            eye_rotation_rate: 100.0,
            reshaping_rate: 0.8420502936726204,
            fov_tweak_rate: 1.1388770853946732,
            eye_center_offsets_change_rate: 1.0,
            eye_radius_change_rate: 1.0,
            learning_smoothness_threshold_logistic_input: 0.590058302151147,
            learning_smoothness_threshold: 0.7649687688799127,
            learning_increase_factor: 1.4423928518524542,
            learning_decrease_factor_logistic_input: -1.1699894812150484,
            learning_decrease_factor: 0.08786560087570966,
        }
    }

    pub fn mutate(&self, rate: f64) -> Self {
        let mut rng = rand::thread_rng();
        let mut result = self.clone();
        result.translation_rate *= rng.gen_range(-rate..=rate).exp();
        result.rotation_rate *= rng.gen_range(-rate..=rate).exp();
        result.reshaping_rate *= rng.gen_range(-rate..=rate).exp();
        result.fov_tweak_rate *= rng.gen_range(-rate..=rate).exp();
        result.eye_rotation_rate *= rng.gen_range(-rate..=rate).exp();
        result.eye_center_offsets_change_rate *= rng.gen_range(-rate..=rate).exp();
        result.eye_radius_change_rate *= rng.gen_range(-rate..=rate).exp();
        result.learning_increase_factor =
            1.0 + (result.learning_increase_factor - 1.0) * rng.gen_range(-rate..=rate).exp();
        result.learning_smoothness_threshold_logistic_input += rng.gen_range(-rate..=rate);
        result.learning_decrease_factor_logistic_input += rng.gen_range(-rate..=rate);
        result.learning_smoothness_threshold =
            0.5 * (result.learning_smoothness_threshold_logistic_input.tanh() + 1.0);
        result.learning_decrease_factor =
            0.5 * (result.learning_decrease_factor_logistic_input.tanh() + 1.0);
        result
    }
}

impl Frame {
    fn rotated_offset(&self, offset: VectorSlice3<f64>) -> Vector3<f64> {
        self.orientation * offset
    }
    fn landmark_position(&self, offset: VectorSlice3<f64>) -> Vector3<f64> {
        self.center_of_mass + self.rotated_offset(offset)
    }
}

pub struct AddFrameResults {
    pub final_loss: f64,
    pub iterations: usize,
}

const ROTATION_DIMENSIONS: [[usize; 3]; 3] = [[0, 1, 2], [1, 2, 0], [2, 0, 1]];

impl FacePositionModel {
    pub fn default_from_camera(camera_landmarks: Arc<CameraLandmarks>) -> Self {
        let mean = camera_landmarks.regular_landmarks.column_mean();
        let landmark_offsets = matrix_from_column_iter(
            camera_landmarks
                .regular_landmarks
                .column_iter()
                .map(|v| v - mean)
                .map(|v| Vector3::new(v[0], v[1], 0.0)),
        );
        let eye_radius = 0.0005;
        let eye_center_offsets = Matrix3x2::from_columns(
            &camera_landmarks
                .pupils
                .column_iter()
                .map(|v| v - mean)
                .map(|v| Vector3::new(v[0], v[1], eye_radius))
                .collect::<Vec<_>>(),
        );
        FacePositionModel {
            frames: vec![Frame {
                time_index: 0,
                camera_landmarks,
                center_of_mass: Vector3::new(0.0, 0.0, 1.0),
                orientation: UnitQuaternion::identity(),
                eye_directions: [Unit::new_unchecked(Vector3::new(0.0, 0.0, -1.0)); 2],
            }],
            landmark_offsets,
            camera_fov_slope: Vector2::new(1.0, 1.0),
            eye_center_offsets,
            eye_radii: Vector2::new(eye_radius, eye_radius),
        }
    }

    fn analyze(
        &self,
        parameters: &MetaParameters,
        last_frame_only: bool,
    ) -> FacePositionModelAnalysis {
        let mut loss = 0.0;
        let mut d_loss_d_fov_slope = Vector2::new(0.0, 0.0);
        // let mut d2_loss_d_fov_slope2 = Vector2::new(0.0, 0.0);
        let mut d_loss_d_landmark_offsets = Matrix3xX::zeros(self.landmark_offsets.ncols());
        let mut d_loss_d_eye_center_offsets = Matrix3x2::zeros();
        let mut d_loss_d_eye_radius_changes = Vector2::zeros();
        let mut landmark_loss = DVector::zeros(self.landmark_offsets.ncols());
        // let mut d2_loss_d_landmark_offsets2 = Matrix3xX::zeros(self.landmark_offsets.ncols());
        let mut frames = Vec::with_capacity(self.frames.len());
        let &[cfx, cfy] = self.camera_fov_slope.as_ref();

        let start = if last_frame_only {
            self.frames.len() - 1
        } else {
            0
        };
        for frame in &self.frames[start..] {
            let mut frame_loss = 0.0;
            let mut d_loss_d_translation = Vector3::zeros();
            // let mut d2_loss_d_translation2 = Vector3::new(0.0, 0.0, 0.0);
            let mut d_loss_d_rotation = Vector3::zeros();
            // let mut d2_loss_d_rotation2 = Vector3::new(0.0, 0.0, 0.0);
            let mut d_loss_d_eye_rotations = Matrix3x2::zeros();
            for (
                camera_landmark,
                landmark_offset,
                mut d_loss_d_landmark_offset,
                //mut d2_loss_d_landmark_offset2,
                landmark_loss,
            ) in multizip((
                frame.camera_landmarks.regular_landmarks.column_iter(),
                self.landmark_offsets.column_iter(),
                d_loss_d_landmark_offsets.column_iter_mut(),
                landmark_loss.iter_mut(),
                //     d2_loss_d_landmark_offsets2.column_iter_mut(),
            )) {
                let rotated_offset = frame.rotated_offset(landmark_offset);
                let model_landmark = frame.center_of_mass + rotated_offset;
                let &[x, y, z] = model_landmark.as_ref();
                let &[cx, cy] = camera_landmark.as_ref();

                // optimizations (avoid duplicate work)
                let recip_z = z.recip();
                let two_over_z2 = 2.0 * recip_z * recip_z;
                let two_over_z3 = two_over_z2 * recip_z;
                // let four_over_z4 = two_over_z2 * two_over_z2;
                let x_cfx = x * cfx;
                let y_cfy = y * cfy;
                let z_cx = z * cx;
                let z_cy = z * cy;
                let two_x_cfx_minus_z_cx_over_z2 = (x_cfx - z_cx) * two_over_z2;
                let two_y_cfy_minus_z_cy_over_z2 = (y_cfy - z_cy) * two_over_z2;

                // "loss is the square of the planar distance between expected and observed camera locations"
                let loss = (x_cfx * recip_z - cx).powi(2) + (y_cfy * recip_z - cy).powi(2);
                frame_loss += loss;
                *landmark_loss += loss;

                // derivatives of the above loss function
                let d_loss_d_landmark_position = Vector3::new(
                    cfx * two_x_cfx_minus_z_cx_over_z2,
                    cfy * two_y_cfy_minus_z_cy_over_z2,
                    ((z_cx - x_cfx) * x + ((z_cy - y_cfy) * y)) * two_over_z3,
                );
                // let d2_loss_d_landmark_position2 = Vector3::new(
                //     cfx.powi(2) * two_over_z2,
                //     cfy.powi(2) * two_over_z2,
                //     (x_cfx * (1.5 * x_cfx - z_cx) + y_cfy * (1.5 * y_cfy - z_cy)) * four_over_z4,
                // );

                d_loss_d_fov_slope += Vector2::new(
                    x * two_x_cfx_minus_z_cx_over_z2,
                    y * two_y_cfy_minus_z_cy_over_z2,
                );

                // d2_loss_d_fov_slope2 +=
                //     Vector2::new(x.powi(2) * two_over_z2, y.powi(2) * two_over_z2);

                // any change to the offset will be rotated by the same amount the offset itself is
                // so a change of dv in landmark_offset is a change of rotation*dv in landmark_position
                // i.e. d_landmark_position = rotation*d_landmark_offset
                // or d_landmark_offset = rotation.inverse() * d_landmark_position
                d_loss_d_landmark_offset +=
                    frame.orientation.inverse() * d_loss_d_landmark_position;
                // d2_loss_d_landmark_offset2 +=
                //     frame.orientation.inverse() * d2_loss_d_landmark_position2;

                d_loss_d_translation += d_loss_d_landmark_position;
                // d2_loss_d_translation2 += d2_loss_d_landmark_position2;

                for ([_axis, d1, d2], d_loss_d_rotation /*, d2_loss_d_rotation2) */) in zip(
                    ROTATION_DIMENSIONS,
                    &mut d_loss_d_rotation, /*, &mut d2_loss_d_rotation2) */
                ) {
                    let u = rotated_offset[d1];
                    let v = rotated_offset[d2];
                    let dldu = d_loss_d_landmark_position[d1];
                    let dldv = d_loss_d_landmark_position[d2];
                    // let d2ldu2 = d2_loss_d_landmark_position2[d1];
                    // let d2ldv2 = d2_loss_d_landmark_position2[d2];
                    *d_loss_d_rotation += u * dldv - v * dldu;
                    // Leave out the terms expressing how the derivative changes due to the rotation
                    // changing direction, because we want to force this derivative to be positive
                    // *d2_loss_d_rotation2 += v * v * d2ldu2 /*- u * dldu*/ + u * u * d2ldv2 /*- v * dldv*/;
                }
            }

            let eye_loss_factor = 1.0; //self.landmark_offsets.ncols() as f64 / 2.0;

            for (
                camera_pupil,
                eye_center_offset,
                &eye_radius,
                eye_direction,
                mut d_loss_d_eye_center_offset,
                d_loss_d_eye_radius_change,
                mut d_loss_d_eye_rotation,
            ) in multizip((
                frame.camera_landmarks.pupils.column_iter(),
                self.eye_center_offsets.column_iter(),
                self.eye_radii.iter(),
                &frame.eye_directions,
                d_loss_d_eye_center_offsets.column_iter_mut(),
                d_loss_d_eye_radius_changes.iter_mut(),
                d_loss_d_eye_rotations.column_iter_mut(),
            )) {
                let rotated_eye_center_offset = frame.rotated_offset(eye_center_offset);
                let center_to_pupil_offset = eye_direction.into_inner() * eye_radius;
                let rotated_pupil_offset = rotated_eye_center_offset + center_to_pupil_offset;
                let model_pupil = frame.center_of_mass + rotated_pupil_offset;

                // TODO: reduce duplicate code with the above
                let &[x, y, z] = model_pupil.as_ref();
                let &[cx, cy] = camera_pupil.as_ref();

                // optimizations (avoid duplicate work)
                let recip_z = z.recip();
                let two_over_z2 = 2.0 * recip_z * recip_z;
                let two_over_z3 = two_over_z2 * recip_z;
                // let four_over_z4 = two_over_z2 * two_over_z2;
                let x_cfx = x * cfx;
                let y_cfy = y * cfy;
                let z_cx = z * cx;
                let z_cy = z * cy;
                let two_x_cfx_minus_z_cx_over_z2 = (x_cfx - z_cx) * two_over_z2;
                let two_y_cfy_minus_z_cy_over_z2 = (y_cfy - z_cy) * two_over_z2;

                // "loss is the square of the planar distance between expected and observed camera locations"
                let loss = ((x_cfx * recip_z - cx).powi(2) + (y_cfy * recip_z - cy).powi(2))
                    * eye_loss_factor;
                frame_loss += loss;

                // derivatives of the above loss function
                let d_loss_d_pupil_position = Vector3::new(
                    cfx * two_x_cfx_minus_z_cx_over_z2,
                    cfy * two_y_cfy_minus_z_cy_over_z2,
                    ((z_cx - x_cfx) * x + ((z_cy - y_cfy) * y)) * two_over_z3,
                ) * eye_loss_factor;

                d_loss_d_fov_slope += Vector2::new(
                    x * two_x_cfx_minus_z_cx_over_z2,
                    y * two_y_cfy_minus_z_cy_over_z2,
                ) * eye_loss_factor;

                d_loss_d_eye_center_offset += frame.orientation.inverse() * d_loss_d_pupil_position;

                d_loss_d_translation += d_loss_d_pupil_position;

                *d_loss_d_eye_radius_change +=
                    d_loss_d_pupil_position.dot(&eye_direction.into_inner());

                for ([_axis, d1, d2], d_loss_d_rotation, d_loss_d_eye_rotation) in multizip((
                    ROTATION_DIMENSIONS,
                    &mut d_loss_d_rotation,
                    &mut d_loss_d_eye_rotation,
                )) {
                    let u = rotated_pupil_offset[d1];
                    let v = rotated_pupil_offset[d2];
                    let dldu = d_loss_d_pupil_position[d1];
                    let dldv = d_loss_d_pupil_position[d2];
                    *d_loss_d_rotation += u * dldv - v * dldu;

                    let u = center_to_pupil_offset[d1];
                    let v = center_to_pupil_offset[d2];
                    *d_loss_d_eye_rotation += u * dldv - v * dldu;
                }
            }

            loss += frame_loss;

            //assert!(d2_loss_d_translation2.iter().all(|&v| v >= 0.0));
            //assert!(d2_loss_d_rotation2.iter().all(|&v| v >= 0.0));

            frames.push(FrameAnalysis {
                loss: frame_loss,
                d_loss_d_translation,
                d_loss_d_rotation,
                d_loss_d_eye_rotations,
                proposed_translation: -d_loss_d_translation * parameters.translation_rate, //.component_div(&d2_loss_d_translation2),
                proposed_rotation: -d_loss_d_rotation * parameters.rotation_rate, //.component_div(&d2_loss_d_rotation2),
                proposed_eye_rotations: -d_loss_d_eye_rotations * parameters.eye_rotation_rate,
            });
        }
        //assert!(d2_loss_d_landmark_offsets2.iter().all(|&v| v >= 0.0));
        //assert!(d2_loss_d_fov_slope2.iter().all(|&v| v >= 0.0));

        let d_loss_d_landmark_offsets_mean = d_loss_d_landmark_offsets.column_mean();
        for mut column in d_loss_d_landmark_offsets.column_iter_mut() {
            column -= d_loss_d_landmark_offsets_mean;
        }
        let proposed_landmark_offsets_change =
            -&d_loss_d_landmark_offsets * parameters.reshaping_rate;
        // let proposed_landmark_offsets_change = matrix_from_column_iter(
        //     d_loss_d_landmark_offsets
        //         .column_iter()
        //         .zip(d2_loss_d_landmark_offsets2.column_iter())
        //         .map(|(d_loss_d_landmark_offset, d2_loss_d_landmark_offset2)| {
        //             (d_loss_d_landmark_offset - d_loss_d_landmark_offsets_mean)
        //             //.component_div(&d2_loss_d_landmark_offset2)
        //         }),
        // );
        let proposed_fov_slope_change = -d_loss_d_fov_slope * parameters.fov_tweak_rate; //.component_div(&d2_loss_d_fov_slope2),
        let proposed_eye_center_offsets_change =
            -d_loss_d_eye_center_offsets * parameters.eye_center_offsets_change_rate;
        let proposed_eye_radius_changes =
            -d_loss_d_eye_radius_changes * parameters.eye_radius_change_rate;

        let mut d_loss_d_learning = frames
            .iter()
            .map(|frame| {
                frame.proposed_translation.dot(&frame.d_loss_d_translation)
                    + frame.proposed_rotation.dot(&frame.d_loss_d_rotation)
            })
            .sum::<f64>();
        if !last_frame_only {
            d_loss_d_learning += proposed_fov_slope_change.dot(&d_loss_d_fov_slope)
                + proposed_landmark_offsets_change.dot(&d_loss_d_landmark_offsets);
        }
        FacePositionModelAnalysis {
            frames,
            landmark_loss,
            loss,
            d_loss_d_fov_slope,
            d_loss_d_landmark_offsets,
            d_loss_d_eye_center_offsets,
            d_loss_d_eye_radius_changes,
            proposed_fov_slope_change,
            proposed_landmark_offsets_change,
            proposed_eye_center_offsets_change,
            proposed_eye_radius_changes,
            d_loss_d_learning,
        }
    }

    pub fn add_frame(
        &mut self,
        parameters: &MetaParameters,
        camera_landmarks: Arc<CameraLandmarks>,
    ) -> AddFrameResults {
        utils::report_frame_started();
        let last_frame = self.frames.last().unwrap();
        let new_frame = Frame {
            time_index: last_frame.time_index + 1,
            camera_landmarks,
            ..*last_frame
        };
        self.frames.push(new_frame);
        let mut analysis = self.analyze(parameters, false);
        // let mut translation = ChangeRunner::new(descend_by_translation);
        // let mut rotation = ChangeRunner::new(descend_by_rotation);
        // let mut reshaping = ChangeRunner::new(descend_by_reshaping);
        // let mut tweaking_fov = ChangeRunner::new(descend_by_tweaking_fov);
        let mut learning_rate;
        let mut iteration = 0;
        let mut slow_descends = 0;
        for last_frame_only in [true, false] {
            learning_rate = 0.01;
            iteration = 0;
            loop {
                let do_reports = !last_frame_only;
                if do_reports {
                    utils::report_iteration_started();
                    utils::report("loss", analysis.loss);
                    utils::report("learning_rate", learning_rate);
                    utils::report(
                        "proposed_descent_kind_magnitudes",
                        proposed_descent_kind_magnitudes(&analysis).as_slice(),
                    );
                    // if self.frames.last().unwrap().time_index < 110 {
                    //     let olr = optimal_learning_rate(parameters, self, &analysis);
                    //     utils::report("optimal_learning_rate", olr);
                    //     learning_rate = olr * 0.9;
                    // }
                }
                //println!("{iteration}: {}", current.loss);
                // translation.apply(self, &mut analysis);
                // if iteration >= 10 {
                //     rotation.apply(self, &mut analysis);
                // }
                // if iteration >= 20 {
                //     tweaking_fov.apply(self, &mut analysis);
                //     reshaping.apply(self, &mut analysis);
                // }
                let infinitesimal_d_loss_d_learning = analysis.d_loss_d_learning;
                assert!(infinitesimal_d_loss_d_learning <= 0.0);
                let new = if last_frame_only {
                    descend_last_frame(&self, &analysis, learning_rate)
                } else {
                    descend(&self, &analysis, learning_rate)
                };
                let new_analysis = new.analyze(parameters, last_frame_only);
                let observed_d_loss = new_analysis.loss - analysis.loss;
                let observed_d_loss_d_learning = observed_d_loss / learning_rate;
                if do_reports {
                    utils::report(
                        "observed_learning_ratio",
                        observed_d_loss_d_learning / infinitesimal_d_loss_d_learning,
                    );
                }
                // if iteration > 1000 {
                //     panic!("Hit some sort of pathological case at iteration {iteration}: {observed_d_loss}, {learning_rate}, {infinitesimal_d_loss_d_learning}");
                // }
                if observed_d_loss_d_learning
                    > infinitesimal_d_loss_d_learning * parameters.learning_smoothness_threshold
                {
                    learning_rate *= parameters.learning_decrease_factor;
                    if learning_rate < 1.0e-100 {
                        panic!("Hit some sort of pathological case at iteration {iteration}: {observed_d_loss_d_learning}, {infinitesimal_d_loss_d_learning}");
                    }
                    //assert!(self.learning_rate > 0.000000001);
                } else {
                    learning_rate *= parameters.learning_increase_factor;
                }
                if observed_d_loss <= 0.0
                    && observed_d_loss > -0.00000001 * self.landmark_offsets.len() as f64
                {
                    slow_descends += 1;
                    if slow_descends > 2 {
                        // println!(
                        //     "Good enough at iteration {iteration}; learning_rate is {learning_rate}"
                        // );
                        *self = new;
                        if last_frame_only {
                            analysis = self.analyze(parameters, false);
                        } else {
                            analysis = new_analysis;
                        }
                        break;
                    }
                } else {
                    slow_descends = 0;
                }
                if new_analysis.loss < analysis.loss {
                    *self = new;
                    analysis = new_analysis;
                }
                // if current.loss < 0.001f64.powi(2) * self.landmarks.len() as f64 {
                //     println!("Good enough at iteration {iteration}");
                //     break;
                // }
                iteration += 1;
            }
        }

        if self.frames.len() > 30 {
            let orientation_difference_ranks =
                utils::ranks(self.frames.iter().zip(analysis.frames.iter()).map(
                    |(frame, frame_analysis)| {
                        self.frames
                            .iter()
                            .filter(|f2| f2.time_index != frame.time_index)
                            .map(|f2| (f2.orientation.inverse() * frame.orientation).angle())
                            .product::<f64>()
                            / frame_analysis.loss
                    },
                ));

            let least_valuable_index = orientation_difference_ranks
                .into_iter()
                .enumerate()
                .min_by_key(|&(index, rank)| usize::max(index, rank))
                .unwrap()
                .0;

            self.frames.remove(least_valuable_index);
        }

        AddFrameResults {
            final_loss: analysis.loss,
            iterations: iteration + 1,
        }
    }

    pub fn draw(&self, window: &mut Window) {
        use kiss3d::nalgebra::Point3;

        let white = Point3::new(1.0, 1.0, 1.0);
        let red = Point3::new(0.5, 0.0, 0.0);
        let analysis = self.analyze(&MetaParameters::new(), false);
        let max_loss = analysis
            .landmark_loss
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let relative_loss = analysis.landmark_loss / max_loss;

        // camera box:
        let [x, y, z] = [
            0.5 / self.camera_fov_slope[0] as f32,
            0.5 / self.camera_fov_slope[1] as f32,
            1.0,
        ];
        let camera_wireframe_points = [
            Point3::new(x, y, z),
            Point3::new(-x, y, z),
            Point3::new(-x, -y, z),
            Point3::new(x, -y, z),
        ];
        for point in camera_wireframe_points {
            window.draw_line(&Point3::new(0.0, 0.0, 0.0), &point, &white);
        }
        let (last_frame, others) = self.frames.split_last().unwrap();
        for frame in others {
            for offset in self.landmark_offsets.column_iter() {
                window.draw_point(&frame.landmark_position(offset).to_kiss(), &red);
            }
            for ((offset, &radius), &direction) in zip(
                zip(self.eye_center_offsets.column_iter(), self.eye_radii.iter()),
                frame.eye_directions.iter(),
            ) {
                let center = frame.landmark_position(offset);
                let pupil = center + direction.into_inner() * radius;
                window.draw_line(
                    &center.to_kiss(),
                    &pupil.to_kiss(),
                    &Point3::new(1.0, 0.0, 0.0),
                );
                window.draw_line(
                    &pupil.to_kiss(),
                    &(pupil + direction.into_inner() * 1.0).to_kiss(),
                    &Point3::new(0.5, 0.0, 0.0),
                );
            }
        }
        for (offset, &relative_loss) in zip(self.landmark_offsets.column_iter(), &relative_loss) {
            let relative_loss = relative_loss as f32;
            window.draw_point(
                &last_frame.landmark_position(offset).to_kiss(),
                &Point3::new(
                    1.0 - relative_loss,
                    1.0 - relative_loss,
                    1.0 - relative_loss * 0.5,
                ),
            );
        }
        for ((offset, &radius), &direction) in zip(
            zip(self.eye_center_offsets.column_iter(), self.eye_radii.iter()),
            last_frame.eye_directions.iter(),
        ) {
            let center = last_frame.landmark_position(offset);
            let pupil = center + direction.into_inner() * radius;
            window.draw_line(
                &center.to_kiss(),
                &pupil.to_kiss(),
                &Point3::new(0.0, 1.0, 0.0),
            );
            window.draw_line(
                &pupil.to_kiss(),
                &(pupil + direction.into_inner() * 1.0).to_kiss(),
                &Point3::new(0.0, 0.5, 0.0),
            );
        }
    }
}

fn descend(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        frames: model
            .frames
            .iter()
            .zip(&analysis.frames)
            .map(|(f, a)| Frame {
                orientation: rotation_quaternion(a.proposed_rotation * learning_rate)
                    * f.orientation,
                center_of_mass: &f.center_of_mass + a.proposed_translation * learning_rate,
                camera_landmarks: f.camera_landmarks.clone(),
                eye_directions: std::array::from_fn(|i| {
                    let r = rotation_quaternion(a.proposed_eye_rotations.column(i) * learning_rate);
                    let mut d = r * f.eye_directions[i];
                    d.renormalize_fast();
                    d
                }),
                ..*f
            })
            .collect(),
        landmark_offsets: &model.landmark_offsets
            + &analysis.proposed_landmark_offsets_change * learning_rate,
        camera_fov_slope: &model.camera_fov_slope
            + &analysis.proposed_fov_slope_change * learning_rate,
        eye_center_offsets: &model.eye_center_offsets
            + &analysis.proposed_eye_center_offsets_change * learning_rate,
        eye_radii: &model.eye_radii + &analysis.proposed_eye_radius_changes * learning_rate,
    }
}

fn rotation_quaternion(amounts: Vector3<f64>) -> UnitQuaternion<f64> {
    let norm = amounts.norm();
    if norm == 0.0 {
        UnitQuaternion::identity()
    } else {
        UnitQuaternion::from_axis_angle(&Unit::new_unchecked(amounts / norm), norm)
    }
}

fn optimal_learning_rate(
    parameters: &MetaParameters,
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
) -> f64 {
    let mut min = 0.0;
    let mut max = 100.0;
    let mut min_analysis = analysis.clone();
    while (max - min) > 0.0001 {
        let mid = (max + min) / 2.0;
        let mid_model = descend(model, analysis, mid);
        let mid_analysis = mid_model.analyze(parameters, false);
        let agreement = -(analysis
            .proposed_landmark_offsets_change
            .dot(&mid_analysis.d_loss_d_landmark_offsets)
            + analysis
                .proposed_fov_slope_change
                .dot(&mid_analysis.d_loss_d_fov_slope)
            + zip(&analysis.frames, &mid_analysis.frames)
                .map(|(first, second)| {
                    first.proposed_translation.dot(&second.d_loss_d_translation)
                        + first.proposed_rotation.dot(&second.d_loss_d_rotation)
                })
                .sum::<f64>());
        if mid_analysis.loss < min_analysis.loss && agreement > 0.0 {
            min = mid;
            min_analysis = mid_analysis;
        } else {
            max = mid;
        }
    }
    min
}

fn descend_last_frame(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    let (f, rest) = model.frames.split_last().unwrap();
    let a = analysis.frames.last().unwrap();
    let new_last = {
        Frame {
            orientation: rotation_quaternion(a.proposed_rotation * learning_rate) * f.orientation,
            center_of_mass: &f.center_of_mass + a.proposed_translation * learning_rate,
            camera_landmarks: f.camera_landmarks.clone(),
            eye_directions: std::array::from_fn(|i| {
                let r = rotation_quaternion(a.proposed_eye_rotations.column(i) * learning_rate);
                let mut d = r * f.eye_directions[i];
                d.renormalize_fast();
                d
            }),
            ..*f
        }
    };
    FacePositionModel {
        frames: rest
            .iter()
            .cloned()
            .chain(std::iter::once(new_last))
            .collect(),
        landmark_offsets: model.landmark_offsets.clone(),
        camera_fov_slope: model.camera_fov_slope.clone(),
        eye_center_offsets: model.eye_center_offsets.clone(),
        eye_radii: model.eye_radii.clone(),
    }
}

fn proposed_descent_kind_magnitudes(analysis: &FacePositionModelAnalysis) -> [f64; 4] {
    let translation = analysis
        .frames
        .iter()
        .map(|frame| frame.proposed_translation.norm_squared())
        .sum::<f64>()
        .sqrt();
    let rotation = analysis
        .frames
        .iter()
        .map(|frame| frame.proposed_rotation.norm_squared())
        .sum::<f64>()
        .sqrt();
    let reshaping = analysis.proposed_landmark_offsets_change.norm();
    let fov = analysis.proposed_fov_slope_change.norm();
    [translation, rotation, reshaping, fov]
}

// fn descend_by_translation(
//     model: &FacePositionModel,
//     analysis: &FacePositionModelAnalysis,
//     learning_rate: f64,
// ) -> FacePositionModel {
//     FacePositionModel {
//         frames: model
//             .frames
//             .iter()
//             .zip(&analysis.frames)
//             .map(|(f, a)| {
//                 let offset = -a.d_loss_d_translation * learning_rate;
//                 Frame {
//                     center_of_mass: f.center_of_mass + offset,
//                     camera_landmarks: f.camera_landmarks.clone(),
//                     ..*f
//                 }
//             })
//             .collect(),
//         landmark_offsets: model.landmark_offsets.clone(),
//         ..*model
//     }
// }
//
// fn descend_by_rotation(
//     model: &FacePositionModel,
//     analysis: &FacePositionModelAnalysis,
//     learning_rate: f64,
// ) -> FacePositionModel {
//     FacePositionModel {
//         frames: model
//             .frames
//             .iter()
//             .zip(&analysis.frames)
//             .map(|(f, a)| {
//                 let radians = -a.d_loss_d_rotation * learning_rate;
//                 let rotation =
//                     UnitQuaternion::from_euler_angles(radians[0], radians[1], radians[2]);
//                 Frame {
//                     orientation: rotation * f.orientation,
//                     camera_landmarks: f.camera_landmarks.clone(),
//                     ..*f
//                 }
//             })
//             .collect(),
//         landmark_offsets: model.landmark_offsets.clone(),
//         ..*model
//     }
// }
//
// fn descend_by_reshaping(
//     model: &FacePositionModel,
//     analysis: &FacePositionModelAnalysis,
//     learning_rate: f64,
// ) -> FacePositionModel {
//     let deriv_mean = analysis.d_loss_d_landmark_offsets.column_mean();
//     FacePositionModel {
//         frames: model.frames.clone(),
//         landmark_offsets: matrix_from_column_iter(
//             model
//                 .landmark_offsets
//                 .column_iter()
//                 .zip(analysis.d_loss_d_landmark_offsets.column_iter())
//                 .map(|(v, d_loss_d_landmark_offset)| {
//                     v - (d_loss_d_landmark_offset - deriv_mean) * learning_rate
//                 }),
//         ),
//         ..*model
//     }
// }
//
// fn descend_by_tweaking_fov(
//     model: &FacePositionModel,
//     analysis: &FacePositionModelAnalysis,
//     learning_rate: f64,
// ) -> FacePositionModel {
//     FacePositionModel {
//         frames: model.frames.clone(),
//         landmark_offsets: model.landmark_offsets.clone(),
//         camera_fov_slope: model.camera_fov_slope - analysis.d_loss_d_fov_slope * learning_rate,
//         ..*model
//     }
// }
