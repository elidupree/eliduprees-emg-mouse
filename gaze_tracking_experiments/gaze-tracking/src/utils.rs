use nalgebra::{Matrix2xX, Matrix3xX, Vector2, Vector3};
use serde::Serialize;
use std::cell::RefCell;
use std::io::BufWriter;

pub type Vector<T, const DIMENSIONS: usize> =
    nalgebra::Vector<T, nalgebra::Const<DIMENSIONS>, nalgebra::ArrayStorage<T, DIMENSIONS, 1>>;

pub trait VectorExt<T: nalgebra::Scalar, const DIMENSIONS: usize> {
    fn as_array(&self) -> &[T; DIMENSIONS];
}

impl<T: nalgebra::Scalar, const DIMENSIONS: usize> VectorExt<T, DIMENSIONS>
    for Vector<T, DIMENSIONS>
{
    fn as_array(&self) -> &[T; DIMENSIONS] {
        self.as_slice().try_into().unwrap()
    }
}

pub trait Vector3Ext {
    fn to_kiss(&self) -> kiss3d::nalgebra::Point3<f32>;
}

impl Vector3Ext for Vector3<f64> {
    fn to_kiss(&self) -> kiss3d::nalgebra::Point3<f32> {
        kiss3d::nalgebra::Point3::new(self[0] as f32, -self[1] as f32, self[2] as f32)
    }
}

pub fn matrix_from_column_iter(iter: impl IntoIterator<Item = Vector3<f64>>) -> Matrix3xX<f64> {
    Matrix3xX::from_columns(&iter.into_iter().collect::<Vec<_>>())
}

pub fn matrix2_from_column_iter(iter: impl IntoIterator<Item = Vector2<f64>>) -> Matrix2xX<f64> {
    Matrix2xX::from_columns(&iter.into_iter().collect::<Vec<_>>())
}

pub fn ranks(iter: impl IntoIterator<Item = f64>) -> Vec<usize> {
    let mut scores: Vec<(usize, f64)> = iter.into_iter().enumerate().collect();
    scores.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());
    let mut ranks: Vec<usize> = scores.iter().map(|_| 0).collect();
    for (rank, (i, _)) in scores.into_iter().enumerate() {
        ranks[i] = rank;
    }
    ranks
}

#[derive(Serialize, Default)]
struct FrameReport {
    iterations: Vec<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Serialize, Default)]
struct Reports {
    frames: Vec<FrameReport>,
}

thread_local! {static REPORTS: RefCell<Option<Reports>>=RefCell:: default()}

pub fn start_recording_reports() {
    REPORTS.with(|reports| {
        *reports.borrow_mut() = Some(Reports::default());
    });
}

pub fn report_frame_started() {
    REPORTS.with(|reports| {
        if let Some(reports) = &mut *reports.borrow_mut() {
            reports.frames.push(FrameReport::default())
        }
    });
}

pub fn report_iteration_started() {
    REPORTS.with(|reports| {
        if let Some(reports) = &mut *reports.borrow_mut() {
            reports
                .frames
                .last_mut()
                .unwrap()
                .iterations
                .push(serde_json::Map::default());
        }
    });
}

pub fn report(key: &str, value: impl Into<serde_json::Value>) {
    REPORTS.with(|reports| {
        if let Some(reports) = &mut *reports.borrow_mut() {
            reports
                .frames
                .last_mut()
                .unwrap()
                .iterations
                .last_mut()
                .unwrap()
                .insert(key.to_string(), value.into());
        }
    });
}

pub fn save_reports() {
    REPORTS.with(|reports| {
        if let Some(reports) = &*reports.borrow() {
            serde_json::to_writer(
                BufWriter::new(std::fs::File::create("../reports.json").unwrap()),
                &*reports,
            )
            .unwrap();
        }
    });
}
