//! This file is not declared by any `mod`. A tree-sitter parse still finds its
//! imports, and they are real references, but nothing builds this file -- so a
//! statement about it should not become an edge.
use crate::facade::User;

pub fn orphan(_user: &User) {}
