use crate::utils::{matrix_from_column_iter, Vector3Ext};
use kiss3d::window::Window;
use nalgebra::{Matrix2xX, Matrix3xX, UnitQuaternion, Vector2, Vector3, VectorSlice3};
use std::iter;
use std::iter::zip;

#[derive(Clone)]
struct Frame {
    time_index: usize,
    camera_landmarks: Matrix2xX<f64>,
    center_of_mass: Vector3<f64>,
    orientation: UnitQuaternion<f64>,
}

pub struct FacePositionModel {
    frames: Vec<Frame>,
    landmark_offsets: Matrix3xX<f64>,

    /// the "spatial depth units per spatial horizontal unit" at 1.0 planar units away from camera center
    /// units are "spatial depth units" * "planar units" / "spatial horizontal units"
    camera_fov_slope: Vector2<f64>,
}

struct FrameAnalysis {
    loss: f64,
    d_loss_d_translation: Vector3<f64>,
    d_loss_d_rotation: Vector3<f64>,
}

struct FacePositionModelAnalysis {
    frames: Vec<FrameAnalysis>,
    loss: f64,
    d_loss_d_fov_slope: Vector2<f64>,
    d_loss_d_landmark_offsets: Matrix3xX<f64>,
}

impl Frame {
    fn rotated_offset(&self, offset: VectorSlice3<f64>) -> Vector3<f64> {
        self.orientation * offset
    }
    fn landmark_position(&self, offset: VectorSlice3<f64>) -> Vector3<f64> {
        self.center_of_mass + self.rotated_offset(offset)
    }
}

const ROTATION_DIMENSIONS: [[usize; 3]; 3] = [[0, 1, 2], [1, 2, 0], [2, 0, 1]];

impl FacePositionModel {
    pub fn default_from_camera(camera_landmarks: Matrix2xX<f64>) -> Self {
        let mean = camera_landmarks.column_mean();
        let landmark_offsets = matrix_from_column_iter(
            camera_landmarks
                .column_iter()
                .map(|v| Vector3::new(v[0] - mean[0], v[1] - mean[1], 1.0)),
        );
        FacePositionModel {
            frames: vec![Frame {
                time_index: 0,
                camera_landmarks,
                center_of_mass: Vector3::new(0.0, 0.0, 1.0),
                orientation: UnitQuaternion::identity(),
            }],
            landmark_offsets,
            camera_fov_slope: Vector2::new(1.0, 1.0),
        }
    }

    fn analyze(&self) -> FacePositionModelAnalysis {
        let mut loss = 0.0;
        let mut d_loss_d_fov_slope = Vector2::new(0.0, 0.0);
        let mut d_loss_d_landmark_offsets = Matrix3xX::zeros(self.landmark_offsets.ncols());
        let mut frames = Vec::with_capacity(self.frames.len());
        let &[cfx, cfy] = self.camera_fov_slope.as_ref();

        for frame in &self.frames {
            let mut frame_loss = 0.0;
            let mut d_loss_d_translation = Vector3::new(0.0, 0.0, 0.0);
            let mut d_loss_d_rotation = Vector3::new(0.0, 0.0, 0.0);
            for ((camera_landmark, landmark_offset), mut d_loss_d_landmark_offset) in zip(
                zip(
                    frame.camera_landmarks.column_iter(),
                    self.landmark_offsets.column_iter(),
                ),
                d_loss_d_landmark_offsets.column_iter_mut(),
            ) {
                let rotated_offset = frame.rotated_offset(landmark_offset);
                let model_landmark = frame.center_of_mass + rotated_offset;
                let &[x, y, z] = model_landmark.as_ref();
                let &[cx, cy] = camera_landmark.as_ref();

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
                frame_loss += (x_cfx / z - cx).powi(2) + (y_cfy / z - cy).powi(2);

                // derivatives of the above loss function
                let d_loss_d_landmark_position = Vector3::new(
                    cfx * two_x_cfx_minus_z_cx_over_z2,
                    cfy * two_y_cfy_minus_z_cy_over_z2,
                    ((z_cx - x_cfx) * x + ((z_cy - y_cfy) * y)) * two_over_z3,
                );

                d_loss_d_fov_slope += Vector2::new(
                    x * two_x_cfx_minus_z_cx_over_z2,
                    y * two_y_cfy_minus_z_cy_over_z2,
                );

                // any change to the offset will be rotated by the same amount the offset itself is
                // so a change of dv in landmark_offset is a change of rotation*dv in landmark_position
                // i.e. d_landmark_position = rotation*d_landmark_offset
                // or d_landmark_offset = rotation.inverse() * d_landmark_position
                d_loss_d_landmark_offset +=
                    frame.orientation.inverse() * d_loss_d_landmark_position;

                d_loss_d_translation += d_loss_d_landmark_position;

                for ([_axis, d1, d2], d_loss_d_rotation) in
                    iter::zip(ROTATION_DIMENSIONS, &mut d_loss_d_rotation)
                {
                    *d_loss_d_rotation += rotated_offset[d1] * d_loss_d_landmark_position[d2];
                    *d_loss_d_rotation -= rotated_offset[d2] * d_loss_d_landmark_position[d1];
                }
            }

            loss += frame_loss;

            frames.push(FrameAnalysis {
                loss: frame_loss,
                d_loss_d_translation,
                d_loss_d_rotation,
            });
        }

        FacePositionModelAnalysis {
            frames,
            loss,
            d_loss_d_fov_slope,
            d_loss_d_landmark_offsets,
        }
    }

    pub fn add_frame(&mut self, camera_landmarks: Matrix2xX<f64>) {
        let last_frame = self.frames.last().unwrap();
        let new_frame = Frame {
            time_index: last_frame.time_index + 1,
            camera_landmarks,
            ..*last_frame
        };
        self.frames.push(new_frame);
        let mut analysis = self.analyze();
        let mut translation = ChangeRunner::new(descend_by_translation);
        let mut rotation = ChangeRunner::new(descend_by_rotation);
        let mut reshaping = ChangeRunner::new(descend_by_reshaping);
        let mut tweaking_fov = ChangeRunner::new(descend_by_tweaking_fov);
        for iteration in 0..100 {
            //println!("{iteration}: {}", current.loss);
            translation.apply(self, &mut analysis);
            if iteration >= 10 {
                rotation.apply(self, &mut analysis);
            }
            if iteration >= 20 {
                tweaking_fov.apply(self, &mut analysis);
                reshaping.apply(self, &mut analysis);
            }
            // if current.loss < 0.001f64.powi(2) * self.landmarks.len() as f64 {
            //     println!("Good enough at iteration {iteration}");
            //     break;
            // }
        }

        if self.frames.len() > 30 {
            let orientation_difference_ranks =
                crate::utils::ranks(self.frames.iter().zip(analysis.frames.iter()).map(
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
    }

    pub fn draw(&self, window: &mut Window) {
        use kiss3d::nalgebra::Point3;

        let white = Point3::new(1.0, 1.0, 1.0);
        let red = Point3::new(0.5, 0.0, 0.0);

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
        }
        for offset in self.landmark_offsets.column_iter() {
            window.draw_point(&last_frame.landmark_position(offset).to_kiss(), &white);
        }
    }
}

struct ChangeRunner<F> {
    change: F,
    learning_rate: f64,
}

impl<F: FnMut(&FacePositionModel, &FacePositionModelAnalysis, f64) -> FacePositionModel>
    ChangeRunner<F>
{
    fn new(change: F) -> Self {
        ChangeRunner {
            change,
            learning_rate: 0.01,
        }
    }
    fn apply(&mut self, current: &mut FacePositionModel, analysis: &mut FacePositionModelAnalysis) {
        let new = (self.change)(current, analysis, self.learning_rate);
        let new_analysis = new.analyze();
        if new_analysis.loss < analysis.loss {
            self.learning_rate *= 1.1;
            *current = new;
            *analysis = new_analysis;
        } else {
            self.learning_rate /= 2.0;
        }
    }
}

fn descend_by_translation(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        frames: model
            .frames
            .iter()
            .zip(&analysis.frames)
            .map(|(f, a)| {
                let offset = -a.d_loss_d_translation * learning_rate;
                Frame {
                    center_of_mass: f.center_of_mass + offset,
                    camera_landmarks: f.camera_landmarks.clone(),
                    ..*f
                }
            })
            .collect(),
        landmark_offsets: model.landmark_offsets.clone(),
        ..*model
    }
}

fn descend_by_rotation(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        frames: model
            .frames
            .iter()
            .zip(&analysis.frames)
            .map(|(f, a)| {
                let radians = -a.d_loss_d_rotation * learning_rate;
                let rotation =
                    UnitQuaternion::from_euler_angles(radians[0], radians[1], radians[2]);
                Frame {
                    orientation: rotation * f.orientation,
                    camera_landmarks: f.camera_landmarks.clone(),
                    ..*f
                }
            })
            .collect(),
        landmark_offsets: model.landmark_offsets.clone(),
        ..*model
    }
}

fn descend_by_reshaping(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        frames: model.frames.clone(),
        landmark_offsets: matrix_from_column_iter(
            model
                .landmark_offsets
                .column_iter()
                .zip(analysis.d_loss_d_landmark_offsets.column_iter())
                .map(|(v, d_loss_d_landmark_offset)| v - d_loss_d_landmark_offset * learning_rate),
        ),
        ..*model
    }
}

fn descend_by_tweaking_fov(
    model: &FacePositionModel,
    analysis: &FacePositionModelAnalysis,
    learning_rate: f64,
) -> FacePositionModel {
    FacePositionModel {
        frames: model.frames.clone(),
        landmark_offsets: model.landmark_offsets.clone(),
        camera_fov_slope: model.camera_fov_slope - analysis.d_loss_d_fov_slope * learning_rate,
        ..*model
    }
}
