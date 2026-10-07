//! Syntax the grammar cannot build a tree for.
//!
//! A file that fails to parse must contribute no edges and must not take the
//! rest of the graph down with it. Tree-sitter recovers rather than failing, so
//! this exercises recovery: whatever it manages to read here is a bonus, and the
//! requirement is that the file is still a node and its neighbours still resolve.
use crate::facade::User

pub fn broken( {{{ !!! ,,,
