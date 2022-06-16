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
        let &[cfx, cfy] = self.camera_fov_slope.as_array();

        for (camera_landmark, model_landmark) in iter::zip(&*camera_landmarks, &*self.landmarks) {
            center_of_mass += model_landmark;
            let &[x, y, z] = model_landmark.as_array();
            let &[cx, cy] = camera_landmark.as_array();

            // "loss is the square of the planar distance between expected and observed camera locations"
            loss += (x * cfx / z - cx).powi(2) + (y * cfy / z - cy).powi(2);

            // optimizations (avoid duplicate work)
            let recip_z = z.recip();
            let two_over_z2 = 2.0 * recip_z * recip_z;
            let two_over_z3 = two_over_z2 * recip_z;
            let two_x_cfx_minus_z_cx_over_z2 = (x * cfx - z * cx) * two_over_z2;
            let two_y_cfy_minus_z_cy_over_z2 = (y * cfy - z * cy) * two_over_z2;

            // derivatives of the above loss function
            let d_loss_d_landmark = Vector3::new(
                cfx * two_x_cfx_minus_z_cx_over_z2,
                cfy * two_y_cfy_minus_z_cy_over_z2,
                (z * (x * cx + y * cy) - (cfx * x.powi(2) + cfy * y.powi(2))) * two_over_z3,
            );
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
            d_loss_d_translation,
            d_loss_d_rotation_about_center_of_mass,
        }
    }

    pub fn conformed_to(&self, camera_landmarks: Arc<[Vector2<f64>]>) -> Self {
        let mut current = self.analyze(camera_landmarks);
        let mut translation = ChangeRunner::new(descend_by_translation);
        translation.apply(&mut current);
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
            let ld1 = landmark[d1] * cos - landmark[d2] * sin;
            let ld2 = landmark[d2] * cos + landmark[d1] * sin;
            landmark[d1] = ld1;
            landmark[d2] = ld2;
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
    todo!()
}
/*

def conformed_to(self, camera_landmarks):
current = ParametersAnalysis(self, camera_landmarks)
moves = [ChangeRunner(MoveHead(d)) for d in range(3)]
rotations = [ChangeRunner(RotateHead([c for c in range(3) if c != d])) for d in range(3)]
reshape = ChangeRunner(ReshapeHead())
for iteration in range(100):
# print(f"Iter {iteration}:")
candidates = moves.copy()
if iteration > 10:
candidates += rotations
if iteration > 20:
candidates += [reshape]
for candidate in candidates:
current.analyze()
current = candidate.apply(current)
if current.loss < 0.001 ** 2 * len(camera_landmarks):
print(f"Good enough at iteration {iteration}")
break

return current.parameters


def apply(self, new_parameters: arameters, current_analysis: ParametersAnalysis, learning_rate):
center = current_analysis.center_of_mass

derivative = np.zeros((3, 3))
replace_submatrix(derivative, self.dimensions, self.dimensions, [
[0, -1],
[1, 0],
])
d_landmarks_d_radians = (
current_analysis.parameters.landmarks - center) @ derivative.transpose()
d_loss_d_radians = np.sum(current_analysis.d_loss_d_landmarks * d_landmarks_d_radians)
radians = -d_loss_d_radians * learning_rate
# print(f"RotateHead {self.dimensions}: {radians:.6f} radians")

rotation = np.eye(3)
replace_submatrix(rotation, self.dimensions, self.dimensions, [
[np.cos(radians), -np.sin(radians)],
[np.sin(radians), np.cos(radians)],
])
new_parameters.landmarks -= center
new_parameters.landmarks = new_parameters.landmarks @ rotation.transpose()
new_parameters.landmarks += center


class MoveHead(ParametersChange):
def __init__(self, dimension):
self.dimension = dimension

def __str__(self):
return f"MoveHead({self.dimension})"

def apply(self, new_parameters: Parameters, current_analysis: ParametersAnalysis, learning_rate):
d_loss_d_distance = np.sum(current_analysis.d_loss_d_landmarks[:, self.dimension])
distance = -d_loss_d_distance * learning_rate
# print(f"MoveHead {self.dimension}: {distance:.6f} distance")

new_parameters.landmarks[:, self.dimension] += distance


class ReshapeHead(ParametersChange):
def __str__(self):
return f"ReshapeHead"

def apply(self, new_parameters: Parameters, current_analysis: ParametersAnalysis, learning_rate):
changes = -current_analysis.d_loss_d_landmarks * learning_rate
# print(f"ReshapeHead: {np.mean(changes):.6f} mean distance")

new_parameters.landmarks += changes
*/
