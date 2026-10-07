pub mod repository;

use crate::facade::User;

/// Declared here and referenced by `core::mod.rs` through `super`, which is the
/// shape that used to close a cycle between a parent and its own child.
pub struct User {
    pub name: String,
}

impl User {
    pub fn greeting(&self) -> String {
        format!("hello {}", self.name)
    }
}
