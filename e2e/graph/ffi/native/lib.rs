//! A PyO3 extension module: the Rust half of the boundary fixture.
//!
//! `myext` exists only as compiled Rust. No Python file in this tree is
//! named `myext`, so every Python import of it is unresolvable inside
//! Python alone -- which is the situation the boundary join exists for.

use pyo3::prelude::*;

mod math;

#[pyclass]
struct Point {
    x: f64,
    y: f64,
}

#[pymodule]
fn myext(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(crate::math::add, m)?)?;
    m.add_class::<Point>()?;
    Ok(())
}
