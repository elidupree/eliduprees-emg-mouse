use crate::utils::VectorExt;
use nalgebra::{Vector2, Vector3};
use std::iter;
use std::sync::Arc;

#[derive(Clone)]
pub struct FacePositionModel {
    landmarks: Arc<[Vector3<f64>]>,

    /// the "spatial depth units per spatial horizontal unit" at 1.0 planar units away from camera center
    /// units are "spatial depth units" * "planar units" / "spatial horizontal units"
    camera_fov_slope: Vector2<f64>,
}

pub struct AnalyzedFacePositionModel {
    model: FacePositionModel,
    camera_landmarks: Arc<[Vector2<f64>]>,
    loss: f64,
    center_of_mass: Vector3<f64>,
    d_loss_d_fov_slope: Vector2<f64>,
    d_loss_d_landmarks: Vec<Vector3<f64>>,
    d_loss_d_translation: Vector3<f64>,
    d_loss_d_rotation_about_center_of_mass: [f64; 3],
}

const ROTATION_DIMENSIONS: [[usize; 2]; 3] = [[0, 1], [1, 2], [2, 0]];

impl FacePositionModel {
    pub fn default_from_camera(camera_landmarks: &[Vector2<f64>]) -> Self {
        FacePositionModel {
            landmarks: camera_landmarks
                .iter()
                .map(|v| Vector3::new(v[0], v[1], 1.0))
                .collect(),
            camera_fov_slope: Vector2::new(1.0, 1.0),
        }
    }

    pub fn analyze(&self, camera_landmarks: Arc<[Vector2<f64>]>) -> AnalyzedFacePositionModel {
        let mut loss = 0.0;
        let mut d_loss_d_fov_slope = Vector2::new(0.0, 0.0);
        let mut center_of_mass = Vector3::new(0.0, 0.0, 0.0);
        let mut d_loss_d_translation = Vector3::new(0.0, 0.0, 0.0);
        let mut d_loss_d_rotation_about_origin = [0.0, 0.0, 0.0];
        let mut d_loss_d_landmarks = Vec::with_capacity(self.landmarks.len());
        let &[cfx, cfy] = self.camera_fov_slope.as_array();

        for (camera_landmark, model_landmark) in iter::zip(&*camera_landmarks, &*self.landmarks) {
            center_of_mass += model_landmark;
            let &[x, y, z] = model_landmark.as_array();
            let &[cx, cy] = camera_landmark.as_array();

            // optimizations (avoid duplicate work)
            let recip_z = z.recip();
            let two_over_z2 = 2.0 * recip_z * recip_z;
            let two_over_z3 = two_over_z2 * recip_z;
            let x_cfx = x * cfx;
            let y_cfy = y * cfy;
            let z_cx = z * cx;
            let z_cy = z * cy;
            let two_x_cfx_minus_z_cx_over_z2 = (x_cfx - z_cx) * two_over_z2;
            let two_y_cfy_minus_z_cy_over_z2 = (y_cfy - z_cy) * two_over_z2;

            // "loss is the square of the planar distance between expected and observed camera locations"
            loss += (x_cfx / z - cx).powi(2) + (y_cfy / z - cy).powi(2);

            // derivatives of the above loss function
            let d_loss_d_landmark = Vector3::new(
                cfx * two_x_cfx_minus_z_cx_over_z2,
                cfy * two_y_cfy_minus_z_cy_over_z2,
                ((z_cx - x_cfx) * x + ((z_cy - y_cfy) * y)) * two_over_z3,
            );
            d_loss_d_landmarks.push(d_loss_d_landmark);
            d_loss_d_fov_slope += Vector2::new(
                x * two_x_cfx_minus_z_cx_over_z2,
                y * two_y_cfy_minus_z_cy_over_z2,
            );

            d_loss_d_translation += d_loss_d_landmark;

            for ([d1, d2], d_loss_d_rotation_about_origin) in
                iter::zip(ROTATION_DIMENSIONS, &mut d_loss_d_rotation_about_origin)
            {
                *d_loss_d_rotation_about_origin += model_landmark[d1] * d_loss_d_landmark[d2];
                *d_loss_d_rotation_about_origin -= model_landmark[d2] * d_loss_d_landmark[d1];
            }
        }

        center_of_mass /= self.landmarks.len() as f64;

        let d_loss_d_rotation_about_center_of_mass = d_loss_d_rotation_about_origin
            .zip(ROTATION_DIMENSIONS)
            .map(|(d_loss_d_rotation_about_origin, [d1, d2])| {
                d_loss_d_rotation_about_origin
                    // same formula as above: we're subtracting out the amount of loss you
                    // get from how the rotation-about-origin would move the center of mass,
                    // leaving only the loss you get from the rotation-not-moving-the-center-of-mass
                    // component
                    - (center_of_mass[d1] * d_loss_d_translation[d2]
                    - center_of_mass[d2] * d_loss_d_translation[d1])
            });

        AnalyzedFacePositionModel {
            model: self.clone(),
            camera_landmarks,
            loss,
            center_of_mass,
            d_loss_d_fov_slope,
            d_loss_d_landmarks,
            d_loss_d_translation,
            d_loss_d_rotation_about_center_of_mass,
        }
    }

    pub fn conformed_to(&self, camera_landmarks: Arc<[Vector2<f64>]>) -> Self {
        let mut current = self.analyze(camera_landmarks);
        let mut translation = ChangeRunner::new(descend_by_translation);
        let mut rotation = ChangeRunner::new(descend_by_rotation);
        let mut reshaping = ChangeRunner::new(descend_by_reshaping);
        let mut tweaking_fov = ChangeRunner::new(descend_by_tweaking_fov);
        for iteration in 0..100 {
            //println!("{iteration}: {}", current.loss);
            translation.apply(&mut current);
            if iteration >= 10 {
                rotation.apply(&mut current);
            }
            if iteration >= 20 {
                tweaking_fov.apply(&mut current);
                reshaping.apply(&mut current);
            }
            if current.loss < 0.001f64.powi(2) * self.landmarks.len() as f64 {
                println!("Good enough at iteration {iteration}");
                break;
            }
        }
        current.model
    }
}

struct ChangeRunner<F> {
    change: F,
    learning_rate: f64,
}

impl<F: FnMut(&AnalyzedFacePositionModel, f64) -> FacePositionModel> ChangeRunner<F> {
    fn new(change: F) -> Self {
        ChangeRunner {
            change,
            learning_rate: 0.01,
        }
    }
    fn apply(&mut self, current: &mut AnalyzedFacePositionModel) {
        let new =
            (self.change)(current, self.learning_rate).analyze(current.camera_landmarks.clone());
        if new.loss < current.loss {
            self.learning_rate *= 1.1;
            *current = new;
        } else {
            self.learning_rate /= 2.0;
        }
    }
}

fn descend_by_translation(
    analysis: &AnalyzedFacePositionModel,
    learning_rate: f64,
) -> FacePositionModel {
    let offset = -analysis.d_loss_d_translation * learning_rate;
    FacePositionModel {
        landmarks: analysis
            .model
            .landmarks
            .iter()
            .map(|v| v + offset)
            .collect(),
        ..analysis.model
    }
}

fn descend_by_rotation(
    analysis: &AnalyzedFacePositionModel,
    learning_rate: f64,
) -> FacePositionModel {
    let mut landmarks: Arc<[Vector3<f64>]> = analysis.model.landmarks.iter().copied().collect();
    let sines_cosines = analysis.d_loss_d_rotation_about_center_of_mass.map(
        |d_loss_d_rotation_about_center_of_mass| {
            let radians = -d_loss_d_rotation_about_center_of_mass * learning_rate;
            (radians.sin(), radians.cos())
        },
    );
    for landmark in Arc::get_mut(&mut landmarks).unwrap() {
        for ((sin, cos), [d1, d2]) in sines_cosines.zip(ROTATION_DIMENSIONS) {
            let relative = *landmark - analysis.center_of_mass;
            landmark[d1] = analysis.center_of_mass[d1] + relative[d1] * cos - relative[d2] * sin;
            landmark[d2] = analysis.center_of_mass[d2] + relative[d2] * cos + relative[d1] * sin;
        }
    }
    FacePositionModel {
        landmarks,
        ..analysis.model
    }
}

fn descend_by_reshaping(
    analysis: &AnalyzedFacePositionModel,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        landmarks: analysis
            .model
            .landmarks
            .iter()
            .zip(&analysis.d_loss_d_landmarks)
            .map(|(v, d_loss_d_landmark)| v - d_loss_d_landmark * learning_rate)
            .collect(),
        ..analysis.model
    }
}

fn descend_by_tweaking_fov(
    analysis: &AnalyzedFacePositionModel,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        landmarks: analysis.model.landmarks.clone(),
        camera_fov_slope: analysis.model.camera_fov_slope
            - analysis.d_loss_d_fov_slope * learning_rate,
    }
}
