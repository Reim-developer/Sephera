//! The functions the `myext` module registers, in their own file -- as a
//! registered function commonly lives somewhere other than the module
//! that registers it.

#[pyfunction]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
