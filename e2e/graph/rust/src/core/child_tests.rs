//! A child module file whose tests glob-import their parent.
//!
//! `use super::*;` at the top of a *file* names the parent module -- the
//! file that declares this one. The same words inside an inline
//! `mod tests { }` name the file itself, which is the case
//! `inline_tests.rs` covers; this file is the other half of that rule.

use super::*;

pub fn checks(_repository: &Repository) {}
