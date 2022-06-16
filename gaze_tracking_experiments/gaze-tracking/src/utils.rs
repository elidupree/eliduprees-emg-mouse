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
