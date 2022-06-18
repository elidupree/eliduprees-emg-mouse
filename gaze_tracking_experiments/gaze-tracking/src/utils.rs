use nalgebra::{Matrix3xX, Vector3};

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

pub fn ranks(iter: impl IntoIterator<Item = f64>) -> Vec<usize> {
    let mut scores: Vec<(usize, f64)> = iter.into_iter().enumerate().collect();
    scores.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());
    let mut ranks: Vec<usize> = scores.iter().map(|_| 0).collect();
    for (rank, (i, _)) in scores.into_iter().enumerate() {
        ranks[i] = rank;
    }
    ranks
}
