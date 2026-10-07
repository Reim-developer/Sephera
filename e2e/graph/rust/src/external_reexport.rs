//! A `pub use` of a crate that lives outside this project.

pub use http;
pub use serde::Serialize;

use http::Request;
use serde::Serialize as Ser;

pub fn build() -> Request<()> {
    Request::new(())
}

pub fn bound<T: Ser>(_value: &T) {}
