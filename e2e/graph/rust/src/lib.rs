//! A crate small enough to read, with every import shape Rust has.

pub mod core;
pub mod facade;
pub mod sibling;
pub mod inline_tests;
pub mod cfg_gated;
pub mod external_reexport;
pub mod grouped;
pub mod glob;
pub mod broken;
pub mod empty;
pub mod not_utf8;
pub mod orphan;
