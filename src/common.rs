use arrayvec::ArrayVec;
use std::fmt::Debug;

//final unwraps are safe because we know that there is enough capacity

pub fn create_array<T: Debug, const SIZE: usize>(closure: impl FnMut(usize) -> T) -> [T; SIZE] {
    (0..SIZE)
        .into_iter()
        .map(closure)
        .collect::<ArrayVec<_, SIZE>>()
        .into_inner()
        .unwrap()
}

pub fn create_result_array<T: Debug, E: Debug, const SIZE: usize>(
    closure: impl FnMut(usize) -> Result<T, E>,
) -> Result<[T; SIZE], E> {
    Ok((0..SIZE)
        .into_iter()
        .map(closure)
        .collect::<Result<ArrayVec<_, SIZE>, _>>()?
        .into_inner()
        .unwrap())
}
